#!/usr/bin/env bash
# Dev-Hilfe: baut, startet eine Kopie des Servers (Port 8099, Daten aus /tmp/nt) und
# macht Headless-Firefox-Screenshots. Aufruf: support/shot.sh board [WxH]
set -euo pipefail
view="${1:-dashboard}"; size="${2:-1440,900}"
cd "$(dirname "$0")/.."
cargo build -q 2>&1 | grep -E "^error" -A5 || true
for p in $(pgrep -x notice); do [ "$(readlink /proc/$p/cwd)" = /tmp/nt ] && kill "$p"; done; sleep 0.3
rm -rf /tmp/nt/prof; mkdir -p /tmp/nt/prof; [ -f /tmp/nt/notice.db ] || cp notice.db contacts.db archive.db config.toml /tmp/nt/
(cd /tmp/nt && setsid "$OLDPWD/target/debug/notice" --port 8099 --config /tmp/nt/config.toml >/tmp/nt/log 2>&1 &)
sleep 1.5
rm -f /tmp/nt/s-"$view".png
timeout 90 firefox --no-remote --profile /tmp/nt/prof --headless --window-size "$size" \
  --screenshot /tmp/nt/s-"$view".png "http://127.0.0.1:8099/?t=$RANDOM#$view" >/dev/null 2>&1 || true
ls /tmp/nt/s-"$view".png
