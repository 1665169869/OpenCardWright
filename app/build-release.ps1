# 一键打包正式版（Tauri MSI / WiX Toolset v3）
#
# 用法：
#   pwsh -NoProfile -File build-release.ps1                    # 正式版
#   pwsh -NoProfile -File build-release.ps1 -Channel dev       # DEV 安装包（可与正式版共存）
#   pwsh -NoProfile -File build-release.ps1 -OutputDir "D:\delivery"
#   pwsh -NoProfile -File build-release.ps1 -SkipCopy
#   pwsh -NoProfile -File build-release.ps1 -WixBin "C:\Program Files (x86)\WiX Toolset v3.14\bin"
#
# 说明：
# - 不覆盖 CARGO_TARGET_DIR / DEBUG，使用 Tauri/Cargo 默认 target；
# - `pnpm tauri build --bundles msi` 由 Tauri 调用 WiX v3 生成 MSI；
# - 默认把 MSI 复制到项目内 app/output（该目录已 gitignore）；
# - **两个通道完全隔离**：identifier、productName（安装目录/快捷方式）、WiX upgradeCode、
#   数据目录（`OpenCardWright` / `OpenCardWright-dev`）都不同，可以同时安装、同时打开；
#   串口是独占资源，两个版本同时在跑时只有一边能连读卡器。

[CmdletBinding()]
param(
    [string]$OutputDir,
    [switch]$SkipCopy,
    [string]$WixBin,
    [ValidateSet('release', 'dev')][string]$Channel = 'release'
)

$ErrorActionPreference = "Stop"
$AppDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $AppDir
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $AppDir "output"
}

$confPath = Join-Path $AppDir "src-tauri\tauri.conf.json"
if (-not (Test-Path -LiteralPath $confPath)) {
    throw "找不到 $confPath，请在 app 目录内运行此脚本"
}
$conf = Get-Content -LiteralPath $confPath -Raw -Encoding UTF8 | ConvertFrom-Json
$version = [string]$conf.version
$productName = [string]$conf.productName
if ([string]::IsNullOrWhiteSpace($version)) {
    throw "tauri.conf.json 缺少 version"
}

# 通道隔离：DEV 用独立配置覆盖 identifier / productName / 窗口标题 / WiX upgradeCode。
# DEV 安装包仍是 release 构建，数据目录与顶栏标签靠编译期变量 OCW_CHANNEL=dev 区分。
$configArgs = @()
if ($Channel -eq 'dev') {
    $devConfPath = Join-Path $AppDir "src-tauri\tauri.dev.conf.json"
    if (-not (Test-Path -LiteralPath $devConfPath)) {
        throw "缺少 DEV 配置：$devConfPath"
    }
    $devConf = Get-Content -LiteralPath $devConfPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $productName = [string]$devConf.productName
    $env:OCW_CHANNEL = 'dev'
    $configArgs = @('--config', 'src-tauri/tauri.dev.conf.json')
} else {
    # 防止上一次 dev 打包的环境变量残留到正式版
    Remove-Item Env:OCW_CHANNEL -ErrorAction SilentlyContinue
}

# Tauri 需要 cargo；本机常不在 PATH，补上 ~/.cargo/bin。
$cargoDir = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path -LiteralPath (Join-Path $cargoDir "cargo.exe")) -and
    -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$cargoDir;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo；请安装 Rust 或把 ~/.cargo/bin 加入 PATH"
}
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    throw "找不到 pnpm；请先安装 pnpm"
}

# 不覆盖 Cargo/Tauri 环境变量，使用默认 target。
Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
Remove-Item Env:DEBUG -ErrorAction SilentlyContinue
$targetRoot = Join-Path $AppDir "src-tauri\target"

# WiX Toolset v3：Tauri MSI 需要 candle.exe / light.exe。
if ([string]::IsNullOrWhiteSpace($WixBin)) {
    foreach ($candidate in @(
        "C:\Program Files (x86)\WiX Toolset v3.14\bin",
        "C:\Program Files\WiX Toolset v3.14\bin",
        "C:\Program Files (x86)\WiX Toolset v3.11\bin",
        "C:\Program Files\WiX Toolset v3.11\bin",
        "C:\Program Files (x86)\WiX Toolset v3.10\bin",
        "C:\Program Files\WiX Toolset v3.10\bin"
    )) {
        if ((Test-Path -LiteralPath (Join-Path $candidate "candle.exe")) -and
            (Test-Path -LiteralPath (Join-Path $candidate "light.exe"))) {
            $WixBin = $candidate
            break
        }
    }
}
if ([string]::IsNullOrWhiteSpace($WixBin)) {
    $cmd = Get-Command candle -ErrorAction SilentlyContinue
    if ($cmd) { $WixBin = Split-Path -Parent $cmd.Source }
}
if ([string]::IsNullOrWhiteSpace($WixBin) -or
    -not (Test-Path -LiteralPath (Join-Path $WixBin "candle.exe")) -or
    -not (Test-Path -LiteralPath (Join-Path $WixBin "light.exe"))) {
    throw "找不到 WiX Toolset v3 的 candle.exe / light.exe；请安装 WiX Toolset v3.14，或用 -WixBin 指定 bin 目录"
}
$env:PATH = "$WixBin;$env:PATH"

Write-Host "==> $Channel 打包：$productName $version"
Write-Host "    app:         $AppDir"
Write-Host "    target:      $targetRoot"
Write-Host "    wix:         $WixBin"
Write-Host "    output:      $(if ($SkipCopy) { '(跳过复制)' } else { $OutputDir })"
Write-Host ""

& pnpm tauri build --bundles msi @configArgs
if ($LASTEXITCODE -ne 0) {
    throw "pnpm tauri build --bundles msi $($configArgs -join ' ') 失败，退出码 $LASTEXITCODE"
}

$msiDir = Join-Path $targetRoot "release\bundle\msi"
$msi = Get-ChildItem -LiteralPath $msiDir -File -Filter "*.msi" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $msi) {
    throw "没有找到 MSI 安装包：$msiDir\*.msi"
}

Write-Host ""
Write-Host "==> 安装包：$($msi.FullName)"
Write-Host "    大小：  $([math]::Round($msi.Length / 1MB, 2)) MiB"

if (-not $SkipCopy) {
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
    $asciiName = if ($Channel -eq 'dev') {
        "OpenCardWright_${version}_dev_x64.msi"
    } else {
        "OpenCardWright_${version}_x64.msi"
    }
    $dest = Join-Path $OutputDir $asciiName
    Copy-Item -LiteralPath $msi.FullName -Destination $dest -Force
    $hash = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash
    Write-Host "    交付副本：$dest"
    Write-Host "    SHA-256： $hash"
}

Write-Host ""
Write-Host "==> 完成"
