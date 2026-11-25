#!/bin/sh
# Packs an .app into <name>.app.tar.gz for the updater (run again after
# stapling the notarization ticket). Usage: archive-macos.sh <path/Name.app>
#
# Same layout the Tauri updater expects (Name.app/…), and no AppleDouble
# (._*) entries or xattr headers: the Tauri updater drops the first path
# component of every entry, so "._Name.app" would land on its extraction
# folder and abort the install.
set -eu
APP="$1"
DIR=$(dirname "$APP")
BASE=$(basename "$APP")
COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --no-acls -C "$DIR" -czf "$APP.tar.gz" "$BASE"
if tar tzf "$APP.tar.gz" | grep -q '/\._\|^\._'; then
  echo "updater archive contains AppleDouble entries" >&2
  exit 1
fi
echo "$APP.tar.gz"
