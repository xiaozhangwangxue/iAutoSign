<#
    Installs the Apple device drivers required to talk to an iPhone from Windows.

    Order of preference:
      1. If "Apple Mobile Device Service" already exists, do nothing.
      2. Try the free "Apple Devices" app from the Microsoft Store via winget
         (small, no iTunes UI).
      3. Fall back to Apple's official iTunes installer, installed silently.

    The script is intentionally chatty: its output ends up in the installer log
    and in %TEMP%\autosign-deps.log so problems can be diagnosed later.
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

function Test-AppleDriverInstalled {
    $service = Get-Service -Name "Apple Mobile Device Service" -ErrorAction SilentlyContinue
    return ($null -ne $service)
}

function Test-BonjourInstalled {
    $service = Get-Service -Name "Bonjour Service" -ErrorAction SilentlyContinue
    return ($null -ne $service)
}

function Start-AppleServices {
    foreach ($name in @("Apple Mobile Device Service", "Bonjour Service")) {
        $service = Get-Service -Name $name -ErrorAction SilentlyContinue
        if ($null -ne $service -and $service.Status -ne "Running") {
            Write-Log "Starting service: $name"
            Start-Service -Name $name -ErrorAction SilentlyContinue
        }
    }
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
$winget = Get-Command winget -ErrorAction SilentlyContinue
if (-not (Test-AppleDriverInstalled) -and ($null -ne $winget)) {
    Write-Log "Trying to install the Apple Devices app from the Microsoft Store..."
    & winget install --id 9NP83LWLPZ9K --source msstore --accept-package-agreements --accept-source-agreements --disable-interactivity 2>&1 |
        ForEach-Object { Write-Log "winget: $_" }

    Start-Sleep -Seconds 5
    if (Test-AppleDriverInstalled) {
        Write-Log "Apple drivers were installed through the Microsoft Store."
        $installed = $true
    } else {
        Write-Log "The Microsoft Store install did not provide the driver; continuing."
    }
}

# ---------------------------------------------------------------
# 2) Apple's official iTunes installer, installed silently
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
