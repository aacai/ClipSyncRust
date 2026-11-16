#!/usr/bin/env bash
# 编 Tauri 版、签名、装进 /Applications；找不到开发证书时退回 adhoc，保证本地能跑。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_SRC="$ROOT/target/release/bundle/macos/ClipSync.app"
APP="/Applications/ClipSync.app"
IDENTITY="${CLIPSYNC_SIGN_IDENTITY:-Apple Development: caicaidarenya@gmail.com (THW9NLMY52)}"

SIGN_IDENTITY="$IDENTITY"
if ! security find-identity -v -p codesigning | grep -Fq "$IDENTITY"; then
  SIGN_IDENTITY="-"
  echo "找不到开发证书 ${IDENTITY}，改用 adhoc 签名（屏幕录制授权可能需要重新确认一次）" >&2
fi

if [[ "${1:-}" == "--build" ]]; then
  cd "$ROOT/apps/clipsync"
  npm run tauri build || echo "打包 dmg 可能失败，但只要 .app 产出就能继续安装" >&2
fi

if [[ ! -d "$APP_SRC" ]]; then
  echo "missing $APP_SRC —— 先 npm run tauri build" >&2
  exit 1
fi

osascript -e 'quit app "ClipSync"' 2>/dev/null || true
osascript -e 'quit app "ClipSyncRust"' 2>/dev/null || true
sleep 0.5
for _ in 1 2 3 4 5; do
  lsof -nP -iTCP:48124 -sTCP:LISTEN >/dev/null 2>&1 || break
  sleep 0.3
done

EXE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP_SRC/Contents/Info.plist")"
codesign --force --sign "$SIGN_IDENTITY" --options runtime \
  "$APP_SRC/Contents/MacOS/$EXE_NAME"
codesign --force --sign "$SIGN_IDENTITY" --options runtime "$APP_SRC"
codesign --verify --strict --deep "$APP_SRC"

rm -rf "$APP"
cp -R "$APP_SRC" "$APP"
touch "$APP"
open "$APP"
echo "installed -> $APP"
