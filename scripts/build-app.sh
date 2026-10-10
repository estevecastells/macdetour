#!/bin/bash
# Build build/MacDetour.app (menu bar UI). Works with Command Line Tools only.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="build/MacDetour.app"

echo "==> Building Swift menu bar app (release)"
(cd app && swift build -c release --product MacDetour)
swift_bin="$(cd app && swift build -c release --show-bin-path)/MacDetour"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$swift_bin" "$app/Contents/MacOS/MacDetour"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>io.github.estevecastells.macdetour.app</string>
  <key>CFBundleName</key><string>MacDetour</string>
  <key>CFBundleExecutable</key><string>MacDetour</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app" >/dev/null
echo "==> Done: $root/$app"
