#!/usr/bin/env bash
# 把刚编好的 clipsync-desktop 换进 /Applications/ClipSyncRust.app；无证书时退回 adhoc。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/clipsync-desktop"
APP="/Applications/ClipSyncRust.app"
EXE="$APP/Contents/MacOS/ClipSyncRust"
IDENTITY="${CLIPSYNC_SIGN_IDENTITY:-Apple Development: caicaidarenya@gmail.com (THW9NLMY52)}"

if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN — run cargo build -p clipsync-desktop first" >&2
  exit 1
fi
if [[ ! -d "$APP" ]]; then
  echo "missing $APP — 这个脚本只做替换，不建包" >&2
  exit 1
fi

SIGN_IDENTITY="$IDENTITY"
if ! security find-identity -v -p codesigning | grep -Fq "$IDENTITY"; then
  SIGN_IDENTITY="-"
  echo "找不到开发证书 ${IDENTITY}，改用 adhoc 签名（屏幕录制授权可能需要重新确认一次）" >&2
fi

osascript -e 'quit app "ClipSyncRust"' 2>/dev/null || true
for _ in 1 2 3 4 5; do
  pgrep -f "$EXE" >/dev/null || break
  sleep 0.3
done
pkill -f "$EXE" 2>/dev/null || true
sleep 0.3

# 绝不就地覆写运行中的 Mach-O：先落 .new，再同目录 rename，否则启动撞 CODESIGNING 击杀。
cp "$BIN" "$EXE.new"
mv -f "$EXE.new" "$EXE"
chmod +x "$EXE"

# Dock 读的是 bundle 里的 icns，光换二进制就还是那张通用贴纸。
cp "$ROOT/crates/clipsync-desktop/packaging/app.icns" "$APP/Contents/Resources/app.icns"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile app.icns" "$APP/Contents/Info.plist" 2>/dev/null ||
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string app.icns" "$APP/Contents/Info.plist"
VER="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VER" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VER" "$APP/Contents/Info.plist"
touch "$APP"

codesign --force --sign "$SIGN_IDENTITY" --options runtime "$APP"
codesign --verify --strict "$APP"
open "$APP"
echo "installed -> $EXE"
