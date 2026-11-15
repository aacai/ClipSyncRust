#!/usr/bin/env bash
# 编 Tauri 版、签好、装进 /Applications。
# 签名固定用同一张开发证书：屏幕录制/麦克风授权认的是「包标识 + 签名」，
# adhoc 每编译一次 cdhash 就变，等于每次重装都要重新授权。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_SRC="$ROOT/apps/clipsync/src-tauri/target/release/bundle/macos/ClipSync.app"
APP="/Applications/ClipSync.app"
IDENTITY="${CLIPSYNC_SIGN_IDENTITY:-Apple Development: caicaidarenya@gmail.com (THW9NLMY52)}"

if ! security find-identity -v -p codesigning | grep -Fq "$IDENTITY"; then
  cat >&2 <<MSG
找不到签名证书：$IDENTITY

登录 Xcode 账号不等于把证书装进钥匙串，还要建一张：
  Xcode → 设置 → 账户 → 选中 caicaidarenya@gmail.com → 管理证书 → 左下 + → Apple Development → 创建
建完这里能看到就对了：
  security find-identity -v -p codesigning
MSG
  exit 1
fi

if [[ "${1:-}" == "--build" ]]; then
  cd "$ROOT/apps/clipsync"
  npm run tauri build
fi

if [[ ! -d "$APP_SRC" ]]; then
  echo "missing $APP_SRC —— 先 npm run tauri build" >&2
  exit 1
fi

osascript -e 'quit app "ClipSync"' 2>/dev/null || true
osascript -e 'quit app "ClipSyncRust"' 2>/dev/null || true
sleep 0.3

# 先签内层二进制，再签外层包；--deep 会把嵌套代码一起签掉。
codesign --force --sign "$IDENTITY" --options runtime \
  "$APP_SRC/Contents/MacOS/ClipSync"
codesign --force --sign "$IDENTITY" --options runtime "$APP_SRC"
codesign --verify --strict --deep "$APP_SRC"
if codesign -dv "$APP_SRC" 2>&1 | grep -Fq 'Signature=adhoc'; then
  echo "签名仍是 adhoc：屏幕录制授权会跟着 cdhash 丢" >&2
  exit 1
fi

rm -rf "$APP"
cp -R "$APP_SRC" "$APP"
touch "$APP"
open "$APP"
echo "installed -> $APP"
