# AML Windows 打包与分发脚本
# 默认直接从 Linux 宿主机同步本体目录代码执行打包，无需通过 GitHub

param(
    [string]$HostBaseUrl = "http://192.168.122.1:8088",
    [switch]$FromGit = $false,
    [string]$CurseForgeApiKey = $env:AML_CURSEFORGE_API_KEY
)

$ErrorActionPreference = "Stop"

$HostSyncUrl = "$HostBaseUrl/sync.zip"
$HostUploadUrl = "$HostBaseUrl/upload"

# 1. 确定虚拟机本地的工作目录
$RepoRoot = "$HOME\AML_Build"
if (-not (Test-Path $RepoRoot)) {
    New-Item -ItemType Directory -Force -Path $RepoRoot | Out-Null
}

if (-not $FromGit) {
    # 直接从 Linux 宿主机读取本体目录代码
    Write-Host ">>> [直读模式] 正在从 Linux 宿主机拉取当前本体代码 ($HostSyncUrl)..." -ForegroundColor Cyan
    $ZipTmp = "$HOME\aml_source_sync.zip"
    try {
        Invoke-WebRequest -Uri $HostSyncUrl -OutFile $ZipTmp -TimeoutSec 60
        $SyncSizeMB = [math]::Round((Get-Item $ZipTmp).Length / 1MB, 2)
        Write-Host ">>> 已下载本体包 ($SyncSizeMB MB)，正在同步解压到 $RepoRoot..." -ForegroundColor Green
        Expand-Archive -Path $ZipTmp -DestinationPath $RepoRoot -Force
        Remove-Item $ZipTmp -Force
    } catch {
        Write-Host "[!] 从宿主机直读失败: $_" -ForegroundColor Yellow
        if (-not (Test-Path (Join-Path $RepoRoot "pubspec.yaml"))) {
            throw "无法从宿主机获取源码且本地无可用代码"
        }
    }
} else {
    # 从 Git 更新
    Write-Host ">>> [Git 模式] 正在通过 Git 更新代码..." -ForegroundColor Cyan
    Set-Location $RepoRoot
    git fetch origin
    git checkout fix/linux-tray-and-java-paths
    git pull origin fix/linux-tray-and-java-paths
}

Set-Location $RepoRoot
Write-Host ">>> 当前工作目录: $RepoRoot" -ForegroundColor Cyan

# 2. 解析当前版本号
$PubspecPath = Join-Path $RepoRoot "pubspec.yaml"
$VersionRaw = (Get-Content $PubspecPath | Select-String -Pattern "^version:\s*(\S+)").Matches.Groups[1].Value.Trim()
$VersionSafe = $VersionRaw -replace '\+', '-'
Write-Host ">>> 检测到版本: $VersionRaw (安全版本名: $VersionSafe)" -ForegroundColor Green

# 3. Flutter 依赖拉取
Write-Host ">>> 执行 flutter pub get..." -ForegroundColor Cyan
flutter pub get

# 4. 编译 Windows Release (Flutter + Rust cargokit)
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

# 5. 打包 WinZip 包
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

# 6. 自动回传至 Linux 宿主机
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
