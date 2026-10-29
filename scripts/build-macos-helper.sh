#!/usr/bin/env bash
# Build what goes into ProxyPilot.app/Contents/Resources/helper (macOS only):
#   sing-box           built from source, universal (arm64 + x86_64)
#   proxypilot-helper  universal
#   manifest.json      version and SHA256 of both, signed with the update key
#                      (manifest.json.sig) so the installed helper can upgrade
#                      itself from a new app without asking for a password.
#
# sing-box is built here because the official darwin assets carry minos 26.0
# and do not start on macOS 12-15. Everything is pinned in tools/sing-box.lock.json.
#
# Needs TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
# Usage: scripts/build-macos-helper.sh
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
lock="$root/tools/sing-box.lock.json"
out="$root/src-tauri/binaries/macos"
work="$root/target/macos-helper"
pick() { jq -r "$1" "$lock"; }

version=$(pick .version)
commit=$(pick .darwin.commit)
go_version=$(pick .darwin.go)
target=$(pick .darwin.deployment_target)
tags=$(pick .darwin.tags)
ldflags=$(pick .darwin.ldflags)
app_version=$(jq -r .version src-tauri/tauri.conf.json)
: "${TAURI_SIGNING_PRIVATE_KEY:?缺少 TAURI_SIGNING_PRIVATE_KEY}"

# resources/ holds only the gitignored license, so a fresh checkout lacks it.
mkdir -p "$work" "$out" "$root/src-tauri/resources"

# --- Go toolchain, checked against the pinned SHA256 -----------------------
case "$(uname -m)" in
  arm64) go_arch=arm64; go_sha=$(pick .darwin.go_sha256_arm64) ;;
  x86_64) go_arch=amd64; go_sha=$(pick .darwin.go_sha256_amd64) ;;
  *) echo "unsupported machine $(uname -m)" >&2; exit 1 ;;
esac
go_tar="$work/go$go_version.darwin-$go_arch.tar.gz"
if [ ! -x "$work/go/bin/go" ]; then
  curl -fsSL -o "$go_tar" "https://go.dev/dl/go$go_version.darwin-$go_arch.tar.gz"
  echo "$go_sha  $go_tar" | shasum -a 256 -c -
  rm -rf "$work/go" && tar -xzf "$go_tar" -C "$work"
fi
export GOROOT="$work/go" GOPATH="$work/gopath" GOCACHE="$work/gocache" GOTOOLCHAIN=local GOFLAGS=-mod=readonly
export PATH="$GOROOT/bin:$PATH"

# --- sing-box at the pinned commit ------------------------------------------
src="$work/sing-box"
if [ ! -d "$src/.git" ]; then
  git init -q "$src"
  git -C "$src" remote add origin "$(pick .darwin.repository)"
fi
git -C "$src" fetch -q --depth 1 origin "$commit"
git -C "$src" checkout -q --force FETCH_HEAD
[ "$(git -C "$src" rev-parse HEAD)" = "$commit" ] || { echo "sing-box commit mismatch" >&2; exit 1; }

for arch in arm64 amd64; do
  (cd "$src" && CGO_ENABLED=1 GOOS=darwin GOARCH=$arch MACOSX_DEPLOYMENT_TARGET=$target \
    go build -trimpath -o "$work/sing-box-$arch" -tags "$tags" \
    -ldflags "-X 'github.com/sagernet/sing-box/constant.Version=$version' $ldflags -s -w -buildid=" ./cmd/sing-box)
done
lipo -create -output "$out/sing-box" "$work/sing-box-arm64" "$work/sing-box-amd64"

# --- the helper -------------------------------------------------------------
export MACOSX_DEPLOYMENT_TARGET=$target
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release -p proxypilot-helper --target "$triple"
done
lipo -create -output "$out/proxypilot-helper" \
  target/aarch64-apple-darwin/release/proxypilot-helper target/x86_64-apple-darwin/release/proxypilot-helper

# --- check, sign, record ----------------------------------------------------
for file in sing-box proxypilot-helper; do
  for arch in arm64 x86_64; do
    minos=$(vtool -arch "$arch" -show-build "$out/$file" | awk '/minos/ {print $2}')
    [ "$minos" = "$target" ] || { echo "$file ($arch) needs macOS $minos, expected $target" >&2; exit 1; }
  done
  codesign --force --sign - "$out/$file"
done
"$out/sing-box" version | head -1

sha() { shasum -a 256 "$1" | awk '{print $1}'; }
jq -cn --arg v "$app_version" --arg h "$(sha "$out/proxypilot-helper")" --arg s "$(sha "$out/sing-box")" \
  '{version: $v, files: {"proxypilot-helper": $h, "sing-box": $s}}' > "$out/manifest.json"
rm -f "$out/manifest.json.sig"
pnpm tauri signer sign "$out/manifest.json" > /dev/null
[ -s "$out/manifest.json.sig" ] || { echo "manifest was not signed" >&2; exit 1; }
cp "$src/LICENSE" "$root/src-tauri/resources/sing-box-LICENSE.txt"
echo "helper files ready in $out"
