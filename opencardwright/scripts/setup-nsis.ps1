#Requires -Version 5.1
<#
.SYNOPSIS
    预置 Tauri 的 NSIS 工具链缓存，绕过 tauri-cli 自带下载器的超时问题。

.DESCRIPTION
    `npm run tauri build --bundles nsis` 会去 GitHub 取 nsis-3.13.zip 和
    nsis_tauri_utils.dll。在本机（以及部分代理环境）这个内置下载器必定
    `timeout: global`，而同样的 URL 用 PowerShell 下载只要两秒。

    本脚本用 PowerShell 下这两个文件、按 tauri-bundler 期望的目录布局摆好，
    校验 SHA1 与必需文件清单，之后 tauri build 会直接复用缓存、不再联网。

    注意布局：工具链要解到 `%LOCALAPPDATA%\tauri\NSIS\` **下面一层**
    （makensis.exe、NSIS.exe、Bin/、Include/ 直接位于 NSIS\ 里），
    不是 `NSIS\nsis-3.13\`。这一层搞错就会被判定"缺文件"后删掉重下。

    校验值对应 tauri-bundler v2（crates/tauri-bundler/src/bundle/windows/nsis/mod.rs）。
    Tauri 升大版本后若打包报 "NSIS directory is missing some files"，重跑本脚本。

.EXAMPLE
    pwsh -NoProfile -File scripts/setup-nsis.ps1
    pwsh -NoProfile -File scripts/setup-nsis.ps1 -Force   # 强制重装
#>
[CmdletBinding()]
param(
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$NsisUrl = 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip'
$NsisSha1 = 'DB14F8AF2EA346B786C6AE19343F61AAF5E09B8E'
$NsisExeSha1 = '6D45D4E8CCF18FE1FDAC9164058F356102A2ECF2'
$UtilsUrl = 'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll'
$UtilsSha1 = '75197FEE3C6A814FE035788D1C34EAD39349B860'

$RequiredFiles = @(
    'makensis.exe',
    'Bin/makensis.exe',
    'Stubs/lzma-x86-unicode',
    'Stubs/lzma_solid-x86-unicode',
    'Plugins/x86-unicode/additional/nsis_tauri_utils.dll',
    'Include/MUI2.nsh',
    'Include/FileFunc.nsh',
    'Include/x64.nsh',
    'Include/nsDialogs.nsh',
    'Include/WinMessages.nsh',
    'Include/Win/COM.nsh',
    'Include/Win/Propkey.nsh',
    'Include/Win/RestartManager.nsh'
)

function Write-Step([string]$m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Write-Ok([string]$m) { Write-Host "    $m" -ForegroundColor Green }

function Assert-Sha1([string]$Path, [string]$Expected, [string]$What) {
    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA1).Hash
    if ($actual -ne $Expected) {
        throw "$What 校验失败`n  期望: $Expected`n  实际: $actual"
    }
    Write-Ok "$What SHA1 匹配"
}

function Test-Installed([string]$Root) {
    foreach ($f in $RequiredFiles) {
        if (-not (Test-Path -LiteralPath (Join-Path $Root $f))) { return $false }
    }
    $exe = Join-Path $Root 'NSIS.exe'
    if (-not (Test-Path -LiteralPath $exe)) { return $false }
    return (Get-FileHash -LiteralPath $exe -Algorithm SHA1).Hash -eq $NsisExeSha1
}

$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$tools = Join-Path $env:LOCALAPPDATA 'tauri'
$nsis = Join-Path $tools 'NSIS'

if (-not $Force -and (Test-Path -LiteralPath $nsis) -and (Test-Installed $nsis)) {
    Write-Ok "NSIS 工具链已就绪: $nsis"
    exit 0
}

New-Item -ItemType Directory -Force $tools | Out-Null

Write-Step '下载 NSIS 3.13'
$zip = Join-Path $tools 'nsis-3.13.zip'
Invoke-WebRequest -Uri $NsisUrl -OutFile $zip -UseBasicParsing -TimeoutSec 600
Assert-Sha1 $zip $NsisSha1 'nsis-3.13.zip'

Write-Step '解压并按 Tauri 期望的层级摆放'
$staging = Join-Path $tools 'nsis-3.13'
Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive -LiteralPath $zip -DestinationPath $tools -Force
if (-not (Test-Path -LiteralPath $staging)) {
    throw "压缩包结构异常：找不到 $staging"
}
if (Test-Path -LiteralPath $nsis) { Remove-Item -LiteralPath $nsis -Recurse -Force }
Move-Item -LiteralPath $staging -Destination $nsis
Assert-Sha1 (Join-Path $nsis 'NSIS.exe') $NsisExeSha1 'NSIS.exe'

Write-Step '安装 nsis_tauri_utils 插件'
$pluginDir = Join-Path $nsis 'Plugins\x86-unicode\additional'
New-Item -ItemType Directory -Force $pluginDir | Out-Null
$plugin = Join-Path $pluginDir 'nsis_tauri_utils.dll'
Invoke-WebRequest -Uri $UtilsUrl -OutFile $plugin -UseBasicParsing -TimeoutSec 600
Assert-Sha1 $plugin $UtilsSha1 'nsis_tauri_utils.dll'

Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue

if (-not (Test-Installed $nsis)) {
    throw '安装后自检未通过：必需文件仍有缺失'
}

Write-Host ''
Write-Step "完成：$nsis"
Write-Host "    现在可以跑 npm run tauri build --bundles nsis（首次打包还会编译 release，约 2–5 分钟）"
