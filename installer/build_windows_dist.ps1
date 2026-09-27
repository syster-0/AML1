# AML Windows 打包与分发脚本
# 支持本地执行与全自动打包/回传

param(
    [string]$HostUploadUrl = "http://192.168.122.1:8088/upload",
    [string]$CurseForgeApiKey = $env:AML_CURSEFORGE_API_KEY
)

$ErrorActionPreference = "Stop"

# 1. 定位工程根目录
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Test-Path (Join-Path $ScriptDir "..\pubspec.yaml")) {
    $RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
} elseif (Test-Path ".\pubspec.yaml") {
    $RepoRoot = (Get-Item ".").FullName
} elseif (Test-Path "$HOME\Documents\GitHub\AML\pubspec.yaml") {
    $RepoRoot = "$HOME\Documents\GitHub\AML"
} else {
    Write-Host "[!] 未找到 AML 工程目录，正在自动克隆最新代码..." -ForegroundColor Yellow
    $RepoRoot = "$HOME\AML"
    if (-not (Test-Path $RepoRoot)) {
        git clone -b fix/linux-tray-and-java-paths https://github.com/syster-0/AML1.git $RepoRoot
    }
}

Set-Location $RepoRoot
Write-Host ">>> 当前工作目录: $RepoRoot" -ForegroundColor Cyan

# 2. 检查并拉取最新分支代码
Write-Host ">>> 正在更新代码..." -ForegroundColor Cyan
git fetch origin
git checkout fix/linux-tray-and-java-paths
git pull origin fix/linux-tray-and-java-paths

# 3. 解析版本号
$PubspecPath = Join-Path $RepoRoot "pubspec.yaml"
$VersionRaw = (Get-Content $PubspecPath | Select-String -Pattern "^version:\s*(\S+)").Matches.Groups[1].Value.Trim()
$VersionSafe = $VersionRaw -replace '\+', '-'
Write-Host ">>> 检测到版本: $VersionRaw (安全版本名: $VersionSafe)" -ForegroundColor Green

# 4. Flutter 依赖拉取
Write-Host ">>> 执行 flutter pub get..." -ForegroundColor Cyan
flutter pub get

# 5. 编译 Windows Release
Write-Host ">>> 开始编译 Windows Release (Flutter + Rust cargokit)..." -ForegroundColor Cyan
$DartDefines = @()
if ($CurseForgeApiKey) {
    $DartDefines += "--dart-define=CURSEFORGE_API_KEY=$CurseForgeApiKey"
}

if ($DartDefines.Count -gt 0) {
    flutter build windows --release $DartDefines
} else {
    flutter build windows --release
}

# 6. 打包 WinZip 包
$OutDirName = "aml-windows-x64-$VersionSafe"
$DistDir = Join-Path $RepoRoot "dist"
$BundleTargetDir = Join-Path $DistDir $OutDirName
$ZipFilePath = Join-Path $DistDir "$OutDirName.zip"
$ReleaseOutputDir = Join-Path $RepoRoot "build\windows\x64\runner\Release"

if (-not (Test-Path $ReleaseOutputDir)) {
    throw "未找到编译产物: $ReleaseOutputDir"
}

if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}

if (Test-Path $BundleTargetDir) {
    Remove-Item -Path $BundleTargetDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $BundleTargetDir | Out-Null

Write-Host ">>> 复制编译文件到发布目录: $BundleTargetDir" -ForegroundColor Cyan
Copy-Item -Path "$ReleaseOutputDir\*" -Destination $BundleTargetDir -Recurse -Force

if (Test-Path $ZipFilePath) {
    Remove-Item -Path $ZipFilePath -Force
}

Write-Host ">>> 正在压缩 WinZip 便携包: $ZipFilePath" -ForegroundColor Cyan
Compress-Archive -Path $BundleTargetDir -DestinationPath $ZipFilePath -Force
Remove-Item -Path $BundleTargetDir -Recurse -Force

$ZipItem = Get-Item $ZipFilePath
$ZipSizeMB = [math]::Round($ZipItem.Length / 1MB, 2)
Write-Host ">>> [成功] 打包完成: $($ZipItem.Name) ($ZipSizeMB MB)" -ForegroundColor Green

# 7. 如果宿主机的接收服务可用，自动回传
if ($HostUploadUrl) {
    Write-Host ">>> 正在尝试将 zip 包自动回传至宿主机 ($HostUploadUrl)..." -ForegroundColor Cyan
    try {
        $Headers = @{ "X-Filename" = $ZipItem.Name }
        $UploadResponse = Invoke-RestMethod -Uri $HostUploadUrl -Method Post -InFile $ZipFilePath -Headers $Headers -TimeoutSec 120
        Write-Host ">>> [回传成功] 宿主机响应: $UploadResponse" -ForegroundColor Green
    } catch {
        Write-Host "[!] 回传至宿主机失败，您也可以在虚拟机内直接使用该 zip 包: $ZipFilePath" -ForegroundColor Yellow
        Write-Host "    错误信息: $_" -ForegroundColor DarkGray
    }
}

Write-Host "=========================================" -ForegroundColor Green
Write-Host "WinZip 包产物路径: $ZipFilePath" -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green
