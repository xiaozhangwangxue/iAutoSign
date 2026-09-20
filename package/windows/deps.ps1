<#
    Installs the Apple device drivers that Windows needs in order to talk to an
    iPhone (Apple Mobile Device Service + Bonjour).

    Detection is deliberately tolerant:
      * newer Apple installers register the service as "AppleMobileDeviceService"
        while older ones use "Apple Mobile Device Service" with spaces, so both
        spellings are accepted;
      * as a functional fallback we also try to connect to the service's port
        (127.0.0.1:27015), which is what actually decides whether AutoSign can
        see a phone.

    The result is written to %TEMP%\autosign-deps-ok.txt or
    %TEMP%\autosign-deps-failed.txt so the installer can report it reliably.
#>

param(
    [string]$LogPath = "$env:TEMP\autosign-deps.log",
    # Directory holding Apple's own packages when they were shipped with the
    # installer (lets the driver be installed without any internet access).
    [string]$BundleDir = ""
)

$ErrorActionPreference = "Continue"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # Older PowerShell versions may not expose the enum member.
}

# Make sure the log directory exists before the first Write-Log call, otherwise
# nothing (including the reason for a failure) would be recorded.
$logDirectory = Split-Path -Parent $LogPath
if ($logDirectory -and -not (Test-Path $logDirectory)) {
    New-Item -ItemType Directory -Force -Path $logDirectory -ErrorAction SilentlyContinue | Out-Null
}

function Write-Log {
    param([string]$Message)
    $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $Message"
    Write-Host $line
    Add-Content -Path $LogPath -Value $line -ErrorAction SilentlyContinue
}

function Get-AppleDriverService {
    Get-Service -ErrorAction SilentlyContinue |
        Where-Object {
            (($_.Name -replace '\s', '') -ieq "AppleMobileDeviceService") -or
            (($_.DisplayName -replace '\s', '') -ieq "AppleMobileDeviceService")
        } |
        Select-Object -First 1
}

function Get-BonjourService {
    Get-Service -ErrorAction SilentlyContinue |
        Where-Object {
            (($_.Name -replace '\s', '') -ieq "Bonjour") -or
            (($_.DisplayName -replace '\s', '') -ieq "BonjourService")
        } |
        Select-Object -First 1
}

# The definitive check: can anything talk to the device service right now?
function Test-UsbmuxPort {
    try {
        $client = New-Object System.Net.Sockets.TcpClient
        $pending = $client.BeginConnect("127.0.0.1", 27015, $null, $null)
        if ($pending.AsyncWaitHandle.WaitOne(2000)) {
            $client.EndConnect($pending)
            $client.Close()
            return $true
        }
        $client.Close()
    } catch {
    }

    return $false
}

function Test-DriverPresent {
    return (($null -ne (Get-AppleDriverService)) -or (Test-UsbmuxPort))
}

function Test-BonjourPresent {
    return ($null -ne (Get-BonjourService))
}

function Start-AppleServices {
    foreach ($service in @((Get-AppleDriverService), (Get-BonjourService))) {
        if ($null -ne $service -and $service.Status -ne "Running") {
            Write-Log "Starting service: $($service.Name)"
            Start-Service -Name $service.Name -ErrorAction SilentlyContinue
        }
    }
}

function Set-Result {
    param([bool]$Success)

    $okPath = Join-Path $env:TEMP "autosign-deps-ok.txt"
    $failPath = Join-Path $env:TEMP "autosign-deps-failed.txt"

    Remove-Item $okPath, $failPath -Force -ErrorAction SilentlyContinue

    if ($Success) {
        Set-Content -Path $okPath -Value (Get-Date -Format s) -ErrorAction SilentlyContinue
    } else {
        Set-Content -Path $failPath -Value (Get-Date -Format s) -ErrorAction SilentlyContinue
    }
}

function Invoke-Winget {
    param([string[]]$Arguments)

    if ($null -eq (Get-Command winget -ErrorAction SilentlyContinue)) {
        Write-Log "winget is not available on this PC."
        return $false
    }

    $common = @("--accept-package-agreements", "--accept-source-agreements")
    # --disable-interactivity only exists on newer winget versions.
    foreach ($variant in @(($Arguments + $common + @("--disable-interactivity")), ($Arguments + $common))) {
        Write-Log ("Running: winget " + ($variant -join " "))
        & winget @variant 2>&1 | ForEach-Object { Write-Log "winget: $_" }
        Write-Log "winget exit code: $LASTEXITCODE"
    }

    return (Test-DriverPresent)
}

function Install-BundledDrivers {
    param([string]$Directory)

    if ([string]::IsNullOrWhiteSpace($Directory) -or -not (Test-Path $Directory)) {
        return $false
    }

    $packages = Get-ChildItem -Path $Directory -Recurse -Filter "*.msi" -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match "AppleMobileDeviceSupport|AppleApplicationSupport|Bonjour" }

    if (-not $packages) {
        Write-Log "No bundled Apple packages found in $Directory"
        return $false
    }

    Write-Log ("Installing bundled Apple packages from " + $Directory)
    $installedSomething = $false

    foreach ($package in $packages) {
        Write-Log "Installing $($package.Name)"
        $process = Start-Process -FilePath "msiexec.exe" `
            -ArgumentList "/i", "`"$($package.FullName)`"", "/qn", "/norestart" `
            -Wait -PassThru
        Write-Log "msiexec exit code: $($process.ExitCode)"

        # 0 = ok, 3010 = ok but needs a reboot, 1638 = a newer version is already there
        if ($process.ExitCode -eq 0 -or $process.ExitCode -eq 3010 -or $process.ExitCode -eq 1638) {
            $installedSomething = $true
        }
    }

    Start-Sleep -Seconds 3
    return $installedSomething
}

function Get-ItunesSetup {
    $setupPath = Join-Path $env:TEMP "iTunes64Setup.exe"

    if (Test-Path $setupPath) {
        Remove-Item $setupPath -Force -ErrorAction SilentlyContinue
    }

    $urls = @(
        "https://www.apple.com/itunes/download/win64",
        "https://secure-appldnld.apple.com/itunes12/iTunes64Setup.exe"
    )

    foreach ($url in $urls) {
        Write-Log "Downloading $url"

        if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
            & curl.exe -L --fail --retry 2 --connect-timeout 20 --output $setupPath $url 2>&1 |
                ForEach-Object { Write-Log "curl: $_" }
        }

        if (-not (Test-Path $setupPath)) {
            try {
                Invoke-WebRequest -Uri $url -OutFile $setupPath -UseBasicParsing
            } catch {
                Write-Log "Download failed: $_"
            }
        }

        if (Test-Path $setupPath) {
            $size = (Get-Item $setupPath).Length
            if ($size -gt 10MB) {
                Write-Log "Downloaded $setupPath ($size bytes)"
                return $setupPath
            }

            Write-Log "Downloaded file is too small ($size bytes); ignoring it"
            Remove-Item $setupPath -Force -ErrorAction SilentlyContinue
        }
    }

    return $null
}

Write-Log "=== AutoSign dependency check started ==="

if ((Test-DriverPresent) -and (Test-BonjourPresent)) {
    Write-Log "Apple Mobile Device Service and Bonjour are already installed."
    Start-AppleServices
    Set-Result $true
    exit 0
}

if (Test-DriverPresent) {
    Write-Log "Device service found, Bonjour is missing (needed for Wi-Fi renewals)."
} else {
    Write-Log "Device service not found."
}

# 0) Packages shipped with AutoSign: no internet, no winget, no Store needed.
if (-not ((Test-DriverPresent) -and (Test-BonjourPresent))) {
    Install-BundledDrivers -Directory $BundleDir | Out-Null
    Start-AppleServices
}

# 1) Microsoft Store "Apple Devices" app (usually the fastest path).
if (-not (Test-DriverPresent)) {
    Write-Log "Trying the Apple Devices app from the Microsoft Store..."
    Invoke-Winget -Arguments @("install", "--id", "9NP83LWLPZ9K", "--source", "msstore") | Out-Null
    Start-Sleep -Seconds 5
}

# 2) Microsoft Store iTunes.
if (-not (Test-DriverPresent)) {
    Write-Log "Trying iTunes from the Microsoft Store..."
    Invoke-Winget -Arguments @("install", "--id", "9PB2MZ1ZMB1S", "--source", "msstore") | Out-Null
    Start-Sleep -Seconds 5
}

# 3) Apple's own installer (also brings Bonjour, which the Store app may skip).
if (-not ((Test-DriverPresent) -and (Test-BonjourPresent))) {
    $setupPath = Get-ItunesSetup

    if ($null -ne $setupPath) {
        Write-Log "Installing Apple's components silently..."
        $process = Start-Process -FilePath $setupPath -ArgumentList "/quiet", "/norestart" -Wait -PassThru
        Write-Log "iTunes installer exited with code $($process.ExitCode)"

        Start-Sleep -Seconds 5

        # 4) Some builds accept /extract; install only what we need.
        if (-not (Test-DriverPresent)) {
            $extractDir = Join-Path $env:TEMP "autosign-itunes-extract"
            New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
            Write-Log "Trying to extract individual packages into $extractDir"
            Start-Process -FilePath $setupPath -ArgumentList "/extract" -WorkingDirectory $extractDir -Wait | Out-Null

            $msis = Get-ChildItem -Path $extractDir -Recurse -Filter "*.msi" -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match "AppleMobileDeviceSupport|AppleApplicationSupport|Bonjour" }

            foreach ($msi in $msis) {
                Write-Log "Installing $($msi.FullName)"
                $msiProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList "/i", "`"$($msi.FullName)`"", "/qn", "/norestart" -Wait -PassThru
                Write-Log "msiexec exited with code $($msiProcess.ExitCode)"
            }
        }

        # 5) Still nothing? Let the user finish Apple's installer by hand.
        if (-not (Test-DriverPresent)) {
            Write-Log "Silent installation did not work; starting Apple's installer with its normal UI."
            Write-Host "请在弹出的 Apple 安装窗口中一路点击下一步完成安装。"
            Start-Process -FilePath $setupPath -Wait | Out-Null
        }
    } else {
        Write-Log "Could not download Apple's installer (network problem?)."
    }
}

Start-AppleServices

if ((Test-DriverPresent) -and (Test-BonjourPresent)) {
    Write-Log "Success: driver and Bonjour are ready."
    Set-Result $true
    exit 0
}

if (Test-DriverPresent) {
    Write-Log "Partial success: driver is present, Bonjour is missing (Wi-Fi renewals may not work)."
    Set-Result $true
    exit 0
}

# 6) Last resort: send the user to the Microsoft Store page.
Write-Log "Failure: device service is still missing; opening the Microsoft Store page."
Start-Process "ms-windows-store://pdp/?productid=9NP83LWLPZ9K" -ErrorAction SilentlyContinue | Out-Null

Write-Host ""
Write-Host "自动安装驱动未成功。可以直接安装 iTunes 解决：https://www.apple.com/itunes/download/win64"
Write-Host "详细日志（发我排查用）：$LogPath"

Set-Result $false
exit 1
