<#
    Collects AutoSign's logs plus a fresh self-check report into a single zip on
    the desktop, so they can be sent for troubleshooting.

    Deliberately does NOT include accounts.json: that file holds the Apple ID
    session token and nobody else needs it to diagnose a problem.
#>

$ErrorActionPreference = "Continue"

$dataDir = Join-Path $env:APPDATA "AutoSign"
$depsLog = Join-Path $env:TEMP "autosign-deps.log"
$desktop = [Environment]::GetFolderPath("Desktop")
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$zipPath = Join-Path $desktop "AutoSign-logs-$stamp.zip"
$staging = Join-Path $env:TEMP "autosign-logs-$stamp"

New-Item -ItemType Directory -Force -Path $staging | Out-Null

# Ask the application for a fresh report first (it also writes selfcheck.txt).
$exe = Join-Path $env:ProgramFiles "AutoSign\AutoSign.exe"
if (Test-Path $exe) {
    Write-Host "正在生成自检报告..."
    & $exe --self-check | Out-Null
}

$collected = 0
foreach ($file in @(
        (Join-Path $dataDir "autosign.log"),
        (Join-Path $dataDir "selfcheck.txt"),
        $depsLog
    )) {
    if (Test-Path $file) {
        Copy-Item $file -Destination $staging -Force
        $collected++
    }
}

if ($collected -eq 0) {
    Write-Host "没有找到日志文件。请先运行一次 AutoSign，然后再试。"
    Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
    exit 1
}

try {
    Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath -Force
} catch {
    Write-Host "打包失败：$_"
    exit 1
}

Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "已收集 $collected 个文件，打包完成："
Write-Host $zipPath

Start-Process explorer.exe "/select,$zipPath" -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
