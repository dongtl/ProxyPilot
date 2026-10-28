# Build a signed release: the installer, its update signature, and the
# latest.json the in-app updater reads from the newest GitHub release.
#
# The signing key never enters the repository. It was generated once with
#   pnpm tauri signer generate -w "$HOME\.tauri\proxypilot.key"
# and must never be regenerated: installed copies only trust its public key.
# Keep it backed up and point -Key at the backup.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/release.ps1 [-Key <path>] [-Notes <text> | -NotesFile <path>]
#
# Normally .github/workflows/release.yml runs this when a version tag is
# pushed, with the key and password from repository secrets. Running it by
# hand is the fallback.
param(
    [string]$Key = (Join-Path $HOME '.tauri\proxypilot.key'),
    [string]$Notes = '',
    [string]$NotesFile = '',
    [string]$Repo = 'dongtl/ProxyPilot'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Read-Utf8([string]$path) { [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8) }

# The version lives in three files; a mismatch would ship an app that
# reports one version while the updater announces another.
$conf = Read-Utf8 (Join-Path $root 'src-tauri\tauri.conf.json')
$version = [regex]::Match($conf, '"version"\s*:\s*"([^"]+)"').Groups[1].Value
$package = [regex]::Match((Read-Utf8 (Join-Path $root 'package.json')), '"version"\s*:\s*"([^"]+)"').Groups[1].Value
$cargo = [regex]::Match((Read-Utf8 (Join-Path $root 'Cargo.toml')), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version -or $version -ne $package -or $version -ne $cargo) {
    throw "版本号不一致：tauri.conf.json=$version，package.json=$package，Cargo.toml=$cargo。请先改成同一个版本。"
}

# The app checks updates against the public key built into it, so the
# private key used here must be its pair. GitHub Actions passes the key and
# its password in from repository secrets; locally they come from -Key and a
# prompt. Either way, the key id check after signing has the final say.
$configured = [regex]::Match($conf, '"pubkey"\s*:\s*"([^"]*)"').Groups[1].Value
$fromEnv = -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)
if ($fromEnv) {
    $env:TAURI_SIGNING_PRIVATE_KEY = $env:TAURI_SIGNING_PRIVATE_KEY.Trim()
    if ([string]::IsNullOrEmpty($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
        throw "已从环境变量读取签名私钥，但没有设置 TAURI_SIGNING_PRIVATE_KEY_PASSWORD。"
    }
} else {
    if (-not (Test-Path $Key)) {
        throw "找不到签名私钥 $Key。请用 -Key 指定备份的私钥路径（.pub 要放在同一个文件夹）。不要重新生成：已发布的版本只认原来那把钥匙。"
    }
    $pubFile = "$Key.pub"
    if (-not (Test-Path $pubFile)) { throw "找不到公钥文件 $pubFile" }
    if ($configured -ne (Read-Utf8 $pubFile).Trim()) {
        throw "$pubFile 和 tauri.conf.json 里的公钥不一致，这不是当初那把私钥。请用 -Key 指向原来备份的私钥；换钥匙会让老用户收不到更新。"
    }
}

if ($NotesFile) { $Notes = (Read-Utf8 $NotesFile).Trim() }
# Without -Notes or -NotesFile, use this version's section of CHANGELOG.md.
# The app shows these notes in its update banner, so keep them plain text.
if (-not $Notes) {
    $changelog = Join-Path $root 'CHANGELOG.md'
    if (Test-Path $changelog) {
        $text = (Read-Utf8 $changelog).Replace("`r`n", "`n")
        $pattern = '(?ms)^##\s*\[?' + [regex]::Escape($version) + '(?![\w.\-]).*?$\s*(.*?)(?=^##\s|\z)'
        $Notes = [regex]::Match($text, $pattern).Groups[1].Value.Trim()
    }
    if (-not $Notes) { Write-Warning "CHANGELOG.md 里没有 $version 这一节，更新提示里不会显示更新内容。" }
}

$hadPassword = Test-Path Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    if (-not $fromEnv) {
        $env:TAURI_SIGNING_PRIVATE_KEY = (Read-Utf8 $Key).Trim()
        if (-not $hadPassword) {
            $secure = Read-Host -AsSecureString "请输入签名私钥的密码"
            $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
            try { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
            finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
        }
    }
    Write-Host "正在构建 ProxyPilot $version（约 10 分钟）…"
    pnpm tauri build --config src-tauri/tauri.release.conf.json
    if ($LASTEXITCODE -ne 0) { throw "构建失败（退出码 $LASTEXITCODE）" }
} finally {
    # Only clear what this script set; in CI the secrets belong to the runner.
    if (-not $fromEnv) {
        Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
        if (-not $hadPassword) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue }
    }
}

$bundle = Join-Path $root 'target\release\bundle\nsis'
$installerName = "ProxyPilot_${version}_x64-setup.exe"
$installer = Join-Path $bundle $installerName
$sigFile = "$installer.sig"
if (-not (Test-Path $installer)) { throw "没有找到安装包 $installer" }
if (-not (Test-Path $sigFile)) { throw "没有找到签名文件 $sigFile，请确认构建时启用了签名" }
$signature = (Read-Utf8 $sigFile).Trim()

# The app requires the signature to name the version it announces
# (requireSignedVersion), so a package without it would never install.
$signedComment = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($signature))
if ($signedComment -notmatch "version:$([regex]::Escape($version))(\s|$)") {
    throw "签名里没有记录版本号 $version，已安装的应用会拒绝这个更新。请升级 @tauri-apps/cli 后重新构建。"
}

# The .pub beside the key could still be paired with a different private
# key, so compare the key id the signature actually carries (bytes 2-9 of
# the second line of both minisign texts) with the configured public key.
function Get-KeyId([string]$encoded) {
    $text = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
    $bytes = [Convert]::FromBase64String(($text -split "`n")[1].Trim())
    [BitConverter]::ToString($bytes, 2, 8)
}
if ((Get-KeyId $signature) -ne (Get-KeyId $configured)) {
    throw "签名用的私钥和 tauri.conf.json 里的公钥不是一对，已安装的应用会拒绝这个更新。请用 -Key 指向原来备份的私钥。"
}

$manifest = [ordered]@{
    version   = $version
    notes     = $Notes
    pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", [Globalization.CultureInfo]::InvariantCulture)
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = $signature
            url       = "https://github.com/$Repo/releases/download/v$version/$installerName"
        }
    }
}
$latest = Join-Path $bundle 'latest.json'
# No BOM: the updater parses this as plain JSON.
[IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding($false)))

$hash = (Get-FileHash -Algorithm SHA256 $installer).Hash
Write-Host ""
Write-Host "构建完成：ProxyPilot $version"
Write-Host "  安装包    $installer"
Write-Host "  更新清单  $latest"
Write-Host "  SHA256    $hash"
if ($env:GITHUB_ACTIONS -ne 'true') {
    Write-Host ""
    Write-Host "手动发布步骤："
    Write-Host "  1. 在 GitHub 新建 Release，标签填 v$version（必须一致，latest.json 里的下载地址用的就是它）"
    Write-Host "  2. 上传上面的安装包和 latest.json 两个文件"
    Write-Host "  3. 不要勾选 Pre-release，然后发布"
}
