#Requires -Version 5.1
<#
.SYNOPSIS
    编译 ../sdk 的 .NET 源码，产出 src-tauri/resources/sdk/，并跑 selftest 作为构建门禁。

.DESCRIPTION
    产品不修改 sdk/，只在构建期用 csc 现编一份到自己的资源目录。
    产物随安装包分发；运行时复制到 %ProgramData% 后执行（见 README §3）。

.PARAMETER BundleDriver
    是否把 LC32RFRW.dll 一并打包。默认 $true。设为 $false 时改为"首启导入驱动"。

.PARAMETER SkipSelfTest
    跳过 27 条黄金向量的门禁。**仅用于调试**，正常构建禁止使用。

.EXAMPLE
    pwsh -NoProfile -File scripts/build-sdk.ps1
    pwsh -NoProfile -File scripts/build-sdk.ps1 -BundleDriver:$false
#>
[CmdletBinding()]
param(
    [bool]$BundleDriver = $true,
    [switch]$SkipSelfTest
)

$ErrorActionPreference = 'Stop'

# csc 的中文诊断按控制台代码页输出；统一成 UTF-8，免得中文警告变成乱码
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

# 已知的驱动版本指纹：结论只对这个哈希成立（见 ../docs/TEMIC门锁卡技术规格.md §14.3）
$KnownDriverSha256 = 'F2F483E08BA8048FFD1C57E66CB35D9E32C1C1E280DC5F2FF100A5D881BCCBC0'

function Write-Step([string]$Message) { Write-Host "==> $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message) { Write-Host "    $Message" -ForegroundColor Green }
function Write-Warn2([string]$Message) { Write-Host "    $Message" -ForegroundColor Yellow }

function Resolve-Csc {
    $candidates = @(
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'),
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe')
    )
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c) { return $c }
    }
    throw '找不到 csc.exe（需要 .NET Framework 4.x）。本脚本只能在 Windows 上运行。'
}

function Invoke-Csc([string]$Csc, [string[]]$Arguments) {
    & $Csc @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "csc 编译失败（退出码 $LASTEXITCODE）：$($Arguments -join ' ')"
    }
}

# ---------------------------------------------------------------- 路径

$scriptDir = $PSScriptRoot
$productRoot = Split-Path -Parent $scriptDir
$repoRoot = Split-Path -Parent $productRoot
$sdkSrc = Join-Path $repoRoot 'sdk'
$outDir = Join-Path $productRoot 'src-tauri\resources\sdk'

Write-Step "仓库根目录: $repoRoot"

foreach ($required in @('PMSLockSDK.cs', 'PMSLockCard.cs', 'LockCard.ini', 'RoomIndex.ini')) {
    $p = Join-Path $sdkSrc $required
    if (-not (Test-Path -LiteralPath $p)) { throw "缺少 SDK 源文件: $p" }
}

# ---------------------------------------------------------------- 清理

if (Test-Path -LiteralPath $outDir) {
    Remove-Item -LiteralPath $outDir -Recurse -Force
}
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

$csc = Resolve-Csc
Write-Ok "csc: $csc"

# ---------------------------------------------------------------- 编译
# /platform:x86 是硬要求：读卡器驱动是 32 位 PE。
# /codepage:65001 保证中文注释与字符串在无 BOM 的 UTF-8 源文件下不乱码。

$common = @('/nologo', '/target:library', '/platform:x86', '/codepage:65001', '/warn:4', '/nowarn:1591')

Write-Step '编译 PMSLockSDK.dll'
$dll = Join-Path $outDir 'PMSLockSDK.dll'
$xml = Join-Path $outDir 'PMSLockSDK.xml'
Invoke-Csc $csc ($common + @("/out:$dll", "/doc:$xml", (Join-Path $sdkSrc 'PMSLockSDK.cs')))
Write-Ok 'PMSLockSDK.dll'

Write-Step '编译 PMSLockCard.exe'
$exe = Join-Path $outDir 'PMSLockCard.exe'
Invoke-Csc $csc (@(
    '/nologo', '/target:exe', '/platform:x86', '/codepage:65001', '/warn:4', '/nowarn:1591',
    "/out:$exe", "/r:$dll", (Join-Path $sdkSrc 'PMSLockCard.cs')
))
Write-Ok 'PMSLockCard.exe'

# 模拟设备用的离线构包器：只调用 SDK 的 CardBuilder / CardNoStore，不碰硬件。
# 有了它，OCW_SIM 下的发卡也会像真机一样推进卡号计数器。
Write-Step '编译 OCWCardBuild.exe（模拟设备离线构包）'
$cardBuild = Join-Path $outDir 'OCWCardBuild.exe'
$cardBuildSrc = Join-Path $productRoot 'tools\CardBuild.cs'
Invoke-Csc $csc (@(
    '/nologo', '/target:exe', '/platform:x86', '/codepage:65001', '/warn:4', '/nowarn:1591',
    "/out:$cardBuild", "/r:$dll", $cardBuildSrc
))
Write-Ok 'OCWCardBuild.exe'

# ---------------------------------------------------------------- 资源

Write-Step '复制运行资源'
Copy-Item -LiteralPath (Join-Path $sdkSrc 'LockCard.ini') -Destination $outDir
Copy-Item -LiteralPath (Join-Path $sdkSrc 'RoomIndex.ini') -Destination $outDir

# 注意：**不复制** sdk/CardNo.state。起始卡号由首启向导确认，避免把现场水位带进安装包。

$driverDir = Join-Path $outDir 'IOServer\LC2000'
$driverSha = $null
if ($BundleDriver) {
    $srcDriver = Join-Path $sdkSrc 'IOServer\LC2000'
    if (-not (Test-Path -LiteralPath $srcDriver)) { throw "缺少驱动目录: $srcDriver" }
    New-Item -ItemType Directory -Path $driverDir -Force | Out-Null
    Copy-Item -Path (Join-Path $srcDriver '*') -Destination $driverDir -Recurse -Force

    $driverDll = Join-Path $driverDir 'LC32RFRW.dll'
    if (-not (Test-Path -LiteralPath $driverDll)) { throw "驱动缺失: $driverDll" }
    $driverSha = (Get-FileHash -LiteralPath $driverDll -Algorithm SHA256).Hash

    if ($driverSha -ne $KnownDriverSha256) {
        Write-Warn2 "驱动哈希与已验证版本不一致！"
        Write-Warn2 "  期望: $KnownDriverSha256"
        Write-Warn2 "  实际: $driverSha"
        Write-Warn2 '  卡格式结论对该版本未必成立，请在测试机跑一次真实读写再发布。'
    } else {
        Write-Ok "驱动指纹匹配 ($driverSha)"
    }
} else {
    Write-Warn2 'BundleDriver=$false：不打包驱动，首启需用户导入。'
}

# ---------------------------------------------------------------- 门禁

if ($SkipSelfTest) {
    Write-Warn2 '已跳过 selftest 门禁（仅供调试，发布构建禁止使用）'
} else {
    Write-Step '门禁：PMSLockCard.exe selftest（27 条黄金向量，无需硬件）'
    Push-Location $outDir
    try {
        & $exe selftest
        $selfTestCode = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($selfTestCode -ne 0) {
        throw "selftest 未通过（退出码 $selfTestCode）。构建中止——卡格式实现与黄金样本不一致。"
    }
    Write-Ok 'selftest 通过'
}

# ---------------------------------------------------------------- manifest

Write-Step '生成 manifest.json'
$entries = @()
Get-ChildItem -LiteralPath $outDir -Recurse -File |
    Where-Object { $_.Name -ne 'manifest.json' } |
    Sort-Object FullName |
    ForEach-Object {
        $entries += [ordered]@{
            path   = $_.FullName.Substring($outDir.Length).TrimStart('\').Replace('\', '/')
            bytes  = $_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
        }
    }

$manifest = [ordered]@{
    product      = 'OpenCardWright'
    sdkSource    = 'sdk/'
    builtAt      = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    bundleDriver = $BundleDriver
    driverSha256 = $driverSha
    driverKnown  = $KnownDriverSha256
    selfTest     = if ($SkipSelfTest) { 'skipped' } else { 'passed' }
    files        = $entries
}

$manifestPath = Join-Path $outDir 'manifest.json'
$json = $manifest | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText($manifestPath, $json, (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Write-Step "完成：$outDir"
$totalBytes = ($entries | ForEach-Object { $_.bytes } | Measure-Object -Sum).Sum
if (-not $totalBytes) { $totalBytes = 0 }
Write-Host ("    文件数 {0}，总计 {1:N1} KB" -f $entries.Count, ($totalBytes / 1KB))
