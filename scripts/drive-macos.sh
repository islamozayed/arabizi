#!/bin/sh
# Runs a dev bundle (built with FEATURES=drive) through a UI script and
# services its screenshot requests. Usage: drive-macos.sh <app> <script> <data-dir>
set -eu
APP="$1"; SCRIPT="$2"; DATA="$3"
rm -f "$SCRIPT.req"
open -n --env FRANCONVERT_DATA_DIR="$DATA" --env FRANCONVERT_DRIVE="$SCRIPT" --env FRANCONVERT_NO_UPDATE_CHECK=1 "$APP"
sleep 1
while pgrep -f "$APP/Contents/MacOS" >/dev/null; do
  if [ -f "$SCRIPT.req" ]; then
    read -r WID OUT < "$SCRIPT.req" || true
    screencapture -x -o -l "$WID" "$OUT"
    rm -f "$SCRIPT.req"
  fi
  sleep 0.05
done
