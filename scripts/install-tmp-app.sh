#!/usr/bin/env bash
# 把刚编好的 clipsync-desktop 装进 /tmp/ClipSyncRust.app（已授屏幕录制的那份）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/clipsync-desktop"
APP="/tmp/ClipSyncRust.app"
EXE="$APP/Contents/MacOS/clipsync-desktop"

if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN — run cargo build -p clipsync-desktop first" >&2
  exit 1
fi

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [[ ! -f "$APP/Contents/Info.plist" ]]; then
  cat >"$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>clipsync-desktop</string>
  <key>CFBundleIdentifier</key>
  <string>local.clipsync.rust.debug</string>
  <key>CFBundleName</key>
  <string>ClipSyncRust</string>
  <key>CFBundleDisplayName</key>
  <string>ClipSyncRust</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleVersion</key>
  <string>0.0.1</string>
  <key>CFBundleShortVersionString</key>
  <string>0.0.1</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST
fi

pkill -f '/tmp/ClipSyncRust.app/Contents/MacOS/clipsync-desktop' 2>/dev/null || true
sleep 0.2
cp "$BIN" "$EXE"
chmod +x "$EXE"
open "$APP"
echo "installed -> $EXE"
