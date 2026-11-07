#!/bin/bash
# Read-only EC RAM snapshot with a tag. Usage: sudo ./scripts/ec_snapshot.sh before
set -e
[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }
[ -n "${1:-}" ] || { echo "usage: $0 TAG"; exit 1; }
modprobe ec_sys
D=${OUT:-./ec-snapshots}; mkdir -p "$D"
dd if=/sys/kernel/debug/ec/ec0/io of="$D/$1.bin" bs=256 count=1 status=none
cat /sys/class/power_supply/BATT/{capacity,status} 2>/dev/null | tr '\n' ' ' > "$D/$1.battery.txt"
[ -n "${SUDO_USER:-}" ] && chown -R "$SUDO_USER": "$D"
echo "saved $D/$1.bin  (battery: $(cat $D/$1.battery.txt))"
