<#
    Installs the Apple device drivers required to talk to an iPhone from Windows.

    Order of preference:
      1. If "Apple Mobile Device Service" and Bonjour are already there, do
         nothing (this is the common case on a PC that has iTunes or iLoader).
      2. Try the free "Apple Devices" app from the Microsoft Store via winget
         (small, no iTunes UI).
      3. Try iTunes from the Microsoft Store via winget.
      4. Download Apple's official iTunes installer and install it silently.
      5. Last resort: run Apple's installer with its normal UI so the user can
         finish the installation themselves.

    The script is intentionally chatty: its output ends up in the installer log
    and in %TEMP%\autosign-deps.log so problems can be diagnosed afterwards.
#>

param(
    [string]$LogPath = "$env:TEMP\autosign-deps.log"
)

$ErrorActionPreference = "Continue"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # Older PowerShell versions may not expose the enum member; TLS 1.2 is the
    # default on Windows 10/11 anyway.
}

function Write-Log {
    param([string]$Message)
    $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $Message"
    Write-Host $line
    Add-Content -Path $LogPath -Value $line -ErrorAction SilentlyContinue
}

# PowerShell's -Name parameter matches the short service name, and the Apple /
# Bonjour packages use different short and display names, so check both.
function Get-AppleService {
    param([string[]]$Names)
    Get-Service -ErrorAction SilentlyContinue |
        Where-Object { $Names -contains $_.Name -or $Names -contains $_.DisplayName } |
        Select-Object -First 1
}

function Test-AppleDriverInstalled {
    return ($null -ne (Get-AppleService -Names @("Apple Mobile Device Service")))
}

function Test-BonjourInstalled {
    return ($null -ne (Get-AppleService -Names @("Bonjour", "Bonjour Service")))
}

function Start-AppleServices {
    foreach ($names in @(@("Apple Mobile Device Service"), @("Bonjour", "Bonjour Service"))) {
        $service = Get-AppleService -Names $names
        if ($null -ne $service -and $service.Status -ne "Running") {
            Write-Log "Starting service: $($service.Name)"
            Start-Service -Name $service.Name -ErrorAction SilentlyContinue
        }
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
    $variants = @(
        ($Arguments + $common + @("--disable-interactivity")),
        ($Arguments + $common)
    )

    foreach ($variant in $variants) {
        Write-Log ("Running: winget " + ($variant -join " "))
        & winget @variant 2>&1 | ForEach-Object { Write-Log "winget: $_" }
        $code = $LASTEXITCODE
        Write-Log "winget exit code: $code"

        if ($code -eq 0) {
            return $true
        }
    }

    return $false
}

Write-Log "AutoSign dependency check started."

if ((Test-AppleDriverInstalled) -and (Test-BonjourInstalled)) {
    Write-Log "Apple Mobile Device Service and Bonjour are already installed."
    Start-AppleServices
    exit 0
}

if (Test-AppleDriverInstalled) {
    Write-Log "Apple Mobile Device Service is present; Bonjour is missing."
} else {
    Write-Log "Apple Mobile Device Service is missing."
}

$installed = $false

# ---------------------------------------------------------------
# 1) Microsoft Store "Apple Devices" app (fastest, no iTunes UI)
# ---------------------------------------------------------------
if (-not (Test-AppleDriverInstalled)) {
    Write-Log "Trying the Apple Devices app from the Microsoft Store..."
    Invoke-Winget -Arguments @("install", "--id", "9NP83LWLPZ9K", "--source", "msstore") | Out-Null

    Start-Sleep -Seconds 5
    if (Test-AppleDriverInstalled) {
        Write-Log "Apple drivers were installed through the Microsoft Store."
        $installed = $true
    } else {
        Write-Log "The Apple Devices app did not provide the driver; trying iTunes."
    }
}

# ---------------------------------------------------------------
# 2) Microsoft Store iTunes through winget
# ---------------------------------------------------------------
if (-not (Test-AppleDriverInstalled)) {
    Write-Log "Trying iTunes from the Microsoft Store..."
    Invoke-Winget -Arguments @("install", "--id", "9PB2MZ1ZMB1S", "--source", "msstore") | Out-Null

    Start-Sleep -Seconds 5
    if (Test-AppleDriverInstalled) {
        Write-Log "Apple drivers were installed through the Microsoft Store (iTunes)."
        $installed = $true
    } else {
        Write-Log "The Microsoft Store install did not provide the driver; continuing."
    }
}

# ---------------------------------------------------------------
# 3) Apple's official iTunes installer, installed silently
# ---------------------------------------------------------------
if (-not $installed) {
    $setupPath = Join-Path $env:TEMP "iTunes64Setup.exe"
    $url = "https://www.apple.com/itunes/download/win64"

    Write-Log "Downloading Apple iTunes (it also contains the device driver) from $url"
    try {
        Invoke-WebRequest -Uri $url -OutFile $setupPath -UseBasicParsing
        $size = (Get-Item $setupPath).Length
        Write-Log "Downloaded $setupPath ($size bytes)"
    } catch {
        Write-Log "Download failed: $_"
        exit 2
    }

    Write-Log "Installing Apple iTunes silently (this installs Apple Mobile Device Support, Apple Application Support and Bonjour) ..."
    $process = Start-Process -FilePath $setupPath -ArgumentList "/quiet", "/norestart" -Wait -PassThru
    Write-Log "iTunes installer exited with code $($process.ExitCode)"

    # Some builds accept /extract; if the full install silently failed we can
    # try to pull the individual MSIs out and install only those.
    Start-Sleep -Seconds 5
    if (-not (Test-AppleDriverInstalled)) {
        $extractDir = Join-Path $env:TEMP "autosign-itunes-extract"
        New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
        Write-Log "Trying to extract individual Apple packages into $extractDir"
        Start-Process -FilePath $setupPath -ArgumentList "/extract" -WorkingDirectory $extractDir -Wait | Out-Null

        $msis = Get-ChildItem -Path $extractDir -Recurse -Filter "*.msi" -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match "AppleMobileDeviceSupport|AppleApplicationSupport|Bonjour" }

        foreach ($msi in $msis) {
            Write-Log "Installing $($msi.FullName)"
            $msiProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList "/i", "`"$($msi.FullName)`"", "/qn", "/norestart" -Wait -PassThru
            Write-Log "msiexec exited with code $($msiProcess.ExitCode)"
        }
    }

    # ---------------------------------------------------------------
    # 4) Last resort: let Apple's installer run with its normal UI
    # ---------------------------------------------------------------
    if (-not (Test-AppleDriverInstalled)) {
        Write-Log "Silent installation did not work; starting Apple's installer with its normal UI."
        Write-Host "Apple 的驱动安装程序需要你手动完成，请在弹出的窗口中一路点击下一步。"
        Start-Process -FilePath $setupPath -Wait | Out-Null
    }
}

Start-AppleServices

if ((Test-AppleDriverInstalled) -and (Test-BonjourInstalled)) {
    Write-Log "Success: Apple drivers and Bonjour are installed."
    exit 0
}

if (Test-AppleDriverInstalled) {
    Write-Log "Partial success: Apple Mobile Device Service is installed but Bonjour is missing (Wi-Fi discovery may not work)."
    exit 0
}

Write-Log "Failure: Apple Mobile Device Service is still missing."
exit 1
