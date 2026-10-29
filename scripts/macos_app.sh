#!/usr/bin/env bash
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Turns a macOS package folder holding the binaries `suhina`, `suhina-cli` and
# `suhina-plugin-host` into one holding `Suhina.app`, the desktop app that Finder opens and that
# can be dragged to Applications (docs/releasing.md › Packages, #799). The three binaries go into
# Suhina.app/Contents/MacOS, side by side, so both programs still find the plug-in host next to
# themselves; `suhina-cli` in the package folder is a link to the one in the app.
#
# The bundle is signed ad hoc (no identity): Apple silicon runs only signed code, and a bundle
# whose parts are signed together opens after the usual warning about an unidentified developer
# instead of being called damaged. Real signing and notarisation come with M8 (ROADMAP).
#
# The app's icon is made from assets/brand/suhina-icon.png at each size macOS asks for.
#
# Usage: scripts/macos_app.sh <package-folder> <version>
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <package-folder> <version>" >&2
  exit 2
fi
package=$1
version=$2
# CFBundleShortVersionString takes up to three numbers only: 0.0.1 of 0.0.1-nightly.20261020+4a2be43.
# The whole version stays in --version and Help › About Suhina.
short=${version%%[-+]*}

app="$package/Suhina.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for bin in suhina suhina-cli suhina-plugin-host; do
  mv "$package/$bin" "$app/Contents/MacOS/$bin"
done

icon="$(dirname "$0")/../assets/brand/suhina-icon.png"
iconset=$(mktemp -d)/Suhina.iconset
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -s format png -z "$size" "$size" "$icon" --out "$iconset/icon_${size}x${size}.png" > /dev/null
  double=$((size * 2))
  sips -s format png -z "$double" "$double" "$icon" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil --convert icns --output "$app/Contents/Resources/Suhina.icns" "$iconset"
rm -rf "$(dirname "$iconset")"

# Suhina › About Suhina is macOS's own panel: the icon, the name, CFBundleShortVersionString and
# NSHumanReadableCopyright, with this file between them. It carries the whole version, as Help ›
# About Suhina does (docs/spec/about.md).
cat > "$app/Contents/Resources/Credits.html" <<EOF
<p style="text-align: center; font-family: -apple-system, sans-serif; font-size: 11px">Version $version<br>GPL-3.0-or-later, free software</p>
EOF

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Suhina</string>
	<key>CFBundleExecutable</key>
	<string>suhina</string>
	<key>CFBundleIconFile</key>
	<string>Suhina</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.suhina-daw.suhina</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Suhina</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$short</string>
	<key>CFBundleVersion</key>
	<string>$short</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.music</string>
	<key>LSMinimumSystemVersion</key>
	<string>${MACOSX_DEPLOYMENT_TARGET:-11.0}</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>© 2026 The Suhina contributors. GPL-3.0-or-later, free software.</string>
	<key>NSMicrophoneUsageDescription</key>
	<string>Suhina records and monitors the audio inputs of its tracks.</string>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"

# Inside out: the helpers first, then the bundle, whose signature covers Info.plist and the app's
# own executable.
codesign --force --sign - "$app/Contents/MacOS/suhina-cli" "$app/Contents/MacOS/suhina-plugin-host"
codesign --force --sign - "$app"
codesign --verify --strict --verbose=2 "$app"

ln -s Suhina.app/Contents/MacOS/suhina-cli "$package/suhina-cli"
