#!/usr/bin/env bash
# macOS counterpart of scripts/release.ps1: build the signed universal app,
# its update package, the dmg, and latest-macos.json, the darwin entries
# .github/workflows/release.yml merges into the release's latest.json.
#
# Needs TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# (the same update key as Windows) and the aarch64/x86_64 Rust targets.
# Usage: scripts/release-macos.sh [owner/repo]
set -euo pipefail
cd "$(dirname "$0")/.."
repo="${1:-dongtl/ProxyPilot}"

version=$(jq -r .version src-tauri/tauri.conf.json)
package=$(jq -r .version package.json)
cargo=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "$version" != "$package" ] || [ "$version" != "$cargo" ]; then
  echo "版本号不一致：tauri.conf.json=$version，package.json=$package，Cargo.toml=$cargo" >&2
  exit 1
fi
# A helper built with the test key would trust anyone holding that key.
if [ -n "${PROXYPILOT_TEST_RELEASE_KEY:-}" ]; then
  echo "PROXYPILOT_TEST_RELEASE_KEY 只用于冒烟测试，正式构建不能设置它" >&2
  exit 1
fi
: "${TAURI_SIGNING_PRIVATE_KEY:?缺少 TAURI_SIGNING_PRIVATE_KEY}"
: "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:?缺少 TAURI_SIGNING_PRIVATE_KEY_PASSWORD}"

# Through bash: a checkout from Windows carries no executable bit.
bash scripts/build-macos-helper.sh
echo "正在构建 ProxyPilot $version（macOS 通用版）…"
pnpm tauri build --target universal-apple-darwin --config src-tauri/tauri.release.conf.json

bundle=target/universal-apple-darwin/release/bundle
tarball="$bundle/macos/ProxyPilot.app.tar.gz"
[ -f "$tarball" ] && [ -f "$tarball.sig" ] || { echo "没有找到更新包或签名 $tarball(.sig)" >&2; exit 1; }
dmg=$(ls "$bundle"/dmg/*.dmg | head -1)
# Versioned names on the release page, like the Windows installer.
dist=target/release-macos
rm -rf "$dist" && mkdir -p "$dist"
update_name="ProxyPilot_${version}_universal.app.tar.gz"
cp "$tarball" "$dist/$update_name"
cp "$dmg" "$dist/ProxyPilot_${version}_universal.dmg"
signature=$(tr -d '\r\n' < "$tarball.sig")

# The same two checks as release.ps1: the version is signed in, and the key
# is the pair of the public key the installed apps carry.
decoded=$(printf '%s' "$signature" | base64 --decode)
printf '%s' "$decoded" | grep -q "version:$version" || { echo "签名里没有记录版本号 $version" >&2; exit 1; }
key_id() { printf '%s' "$1" | base64 --decode | sed -n 2p | base64 --decode | xxd -p -s 2 -l 8; }
configured=$(jq -r .plugins.updater.pubkey src-tauri/tauri.conf.json)
[ "$(key_id "$signature")" = "$(key_id "$configured")" ] || { echo "签名用的私钥和 tauri.conf.json 里的公钥不是一对" >&2; exit 1; }

url="https://github.com/$repo/releases/download/v$version/$update_name"
# A universal app serves both architectures from one package.
jq -n --arg s "$signature" --arg u "$url" \
  '{platforms: {"darwin-aarch64": {signature: $s, url: $u}, "darwin-x86_64": {signature: $s, url: $u}}}' > "$dist/latest-macos.json"
ls -l "$dist"
