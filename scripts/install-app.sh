#!/usr/bin/env bash
# 把刚编好的 clipsync-desktop 直接换进 /Applications/ClipSyncRust.app。
# 签名固定用同一张开发证书：屏幕录制/麦克风授权认的是「包标识 + 证书」，
# adhoc 每编译一次 cdhash 就变，等于每次重装都要重新授权。
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
if ! security find-identity -v -p codesigning | grep -Fq "$IDENTITY"; then
  echo "找不到签名证书：$IDENTITY —— 换证书等于让系统重新问一遍授权" >&2
  exit 1
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
touch "$APP"

codesign --force --sign "$IDENTITY" "$APP"
codesign --verify --strict "$APP"
open "$APP"
echo "installed -> $EXE"
