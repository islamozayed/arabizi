#!/bin/sh
# Builds Franconvert.app (and the updater's Franconvert.app.tar.gz) from the
# release binary. Usage: scripts/bundle-macos.sh [target-triple] [out-dir]
#   BUNDLE_ID / BUNDLE_NAME override the identity (side-by-side test builds).
set -eu
cd "$(dirname "$0")/.."
TARGET="${1:-}"
OUT="${2:-target/bundle}"
NAME="${BUNDLE_NAME:-Franconvert}"
ID="${BUNDLE_ID:-com.franconvert.app}"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)

if [ -n "$TARGET" ]; then
  cargo build --release --locked -p franconvert --target "$TARGET"
  BIN="target/$TARGET/release/franconvert"
else
  cargo build --release -p franconvert ${FEATURES:+--features "$FEATURES"}
  BIN="target/release/franconvert"
fi

APP="$OUT/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/fonts"
cp "$BIN" "$APP/Contents/MacOS/franconvert"
cp app/icons/icon.icns "$APP/Contents/Resources/icon.icns"
cp app/ui/fonts/*.ttf "$APP/Contents/Resources/fonts/"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleExecutable</key><string>franconvert</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>$ID</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>Franconvert pastes the Arabic text into the app you were typing in.</string>
</dict>
</plist>
PLIST

# Sign: Developer ID with hardened runtime + timestamp in CI (needed for
# notarization), ad hoc for local builds.
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --entitlements app/entitlements.plist --sign "$APPLE_SIGNING_IDENTITY" "$APP"
else
  codesign --force --sign - "$APP"
fi

# Updater archive: same layout the Tauri updater expects (Franconvert.app/…).
# No AppleDouble (._*) entries or xattr headers: the Tauri updater drops the
# first path component of every entry, so "._Franconvert.app" would land on
# its extraction folder and abort the install.
COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --no-acls -C "$OUT" -czf "$OUT/$NAME.app.tar.gz" "$NAME.app"
if tar tzf "$OUT/$NAME.app.tar.gz" | grep -q '\._'; then
  echo "updater archive contains AppleDouble entries" >&2
  exit 1
fi
echo "$APP"
