# Download the pinned sing-box release, verify it, and place it where Tauri
# expects the sidecar. The SHA256 in tools/sing-box.lock.json is recorded on
# the first download (upstream publishes no checksum file) and enforced after
# that, so a changed asset fails the build instead of shipping silently.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/fetch-sing-box.ps1 [-Force]
param([switch]$Force)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$lockPath = Join-Path $root 'tools\sing-box.lock.json'
$lock = Get-Content -Raw -Encoding utf8 $lockPath | ConvertFrom-Json

$triple = 'x86_64-pc-windows-msvc'
$binDir = Join-Path $root 'src-tauri\binaries'
$resDir = Join-Path $root 'src-tauri\resources'
$target = Join-Path $binDir "sing-box-$triple.exe"
$license = Join-Path $resDir 'sing-box-LICENSE.txt'
$stamp = Join-Path $binDir 'sing-box.version'

if (-not $Force -and (Test-Path $target) -and (Test-Path $license) -and (Test-Path $stamp) -and
    ((Get-Content -Raw $stamp).Trim() -eq $lock.version)) {
    Write-Host "sing-box $($lock.version) already present"
    exit 0
}

New-Item -ItemType Directory -Force $binDir, $resDir | Out-Null
$work = Join-Path ([IO.Path]::GetTempPath()) "proxypilot-sing-box-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force $work | Out-Null
try {
    $zip = Join-Path $work $lock.asset
    Write-Host "Downloading $($lock.url)"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $lock.url -OutFile $zip -UseBasicParsing

    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    if ([string]::IsNullOrEmpty($lock.sha256)) {
        $lock.sha256 = $hash
        $lock | ConvertTo-Json | Set-Content -Encoding utf8 $lockPath
        Write-Host "Recorded SHA256 $hash in tools/sing-box.lock.json (first download)"
    } elseif ($lock.sha256 -ne $hash) {
        throw "SHA256 mismatch for $($lock.asset): expected $($lock.sha256), got $hash"
    }

    Expand-Archive -Path $zip -DestinationPath $work -Force
    $exe = Get-ChildItem -Path $work -Recurse -Filter 'sing-box.exe' | Select-Object -First 1
    if (-not $exe) { throw 'sing-box.exe not found in the archive' }
    Copy-Item $exe.FullName $target -Force

    $lic = Get-ChildItem -Path $work -Recurse -File | Where-Object { $_.Name -match '^LICEN[CS]E' } | Select-Object -First 1
    if ($lic) {
        Copy-Item $lic.FullName $license -Force
    } else {
        # Fall back to the license at the pinned tag, so the bundle always carries one.
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/SagerNet/sing-box/v$($lock.version)/LICENSE" -OutFile $license -UseBasicParsing
    }
    Set-Content -NoNewline -Encoding ascii $stamp $lock.version
    Write-Host "sing-box $($lock.version) -> $target"
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
