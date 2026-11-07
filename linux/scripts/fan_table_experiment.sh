#!/bin/bash
# Fan-table experiment for the Minisforum V3.
# Switches thermal mode with the vendor command (0xDE 1/2/3, the same one the BIOS / Minisforum Space use),
# dumps EC RAM (read-only) after each switch, then restores the original mode.
# Writes nothing to EC RAM itself. Run as root:
#   sudo ./scripts/fan_table_experiment.sh
set -u
OUT=${OUT:-./ec-modes}
EC=/sys/kernel/debug/ec/ec0/io
PP=/sys/firmware/acpi/platform_profile
PARAM=/sys/module/minisforum_v3_wmi/parameters/allow_profile_set
SETTLE=${SETTLE:-45}      # seconds to wait after each switch
SAMPLES=${SAMPLES:-3}     # dumps per mode, 10 s apart

[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }
lsmod | grep -q '^minisforum_v3_wmi' || { echo "minisforum_v3_wmi not loaded"; exit 1; }
modprobe ec_sys || exit 1
[ -r "$EC" ] || { echo "no $EC"; exit 1; }
mkdir -p "$OUT"

ORIG=$(cat $PP); ORIG_PARAM=$(cat $PARAM)
restore() {
  echo; echo "== restoring mode '$ORIG' and allow_profile_set=$ORIG_PARAM"
  echo "$ORIG" > $PP 2>/dev/null || echo "!! could not restore mode, set it manually: echo $ORIG > $PP"
  sleep 2; echo "   mode now: $(cat $PP)"
  [ "$ORIG_PARAM" = Y ] || echo 0 > $PARAM
  [ -n "${SUDO_USER:-}" ] && chown -R "$SUDO_USER": "$OUT"
}
trap restore EXIT
trap 'exit 130' INT TERM

H=""; for h in /sys/class/hwmon/hwmon*; do [ "$(cat $h/name)" = minisforum_v3 ] && H=$h; done
snap() {  # $1 = tag
  local f="$OUT/$1.bin"
  dd if=$EC of="$f" bs=256 count=1 status=none
  printf "%-22s fan1=%5s fan2=%5s apu=%3s sys=%3s  table: %s\n" "$1" \
    "$(cat $H/fan1_input 2>/dev/null)" "$(cat $H/fan2_input 2>/dev/null)" \
    "$(( $(cat $H/temp1_input) / 1000 ))" "$(( $(cat $H/temp8_input 2>/dev/null || echo 0) / 1000 ))" \
    "$(xxd -s 0x10 -l 31 -p "$f")" | tee -a "$OUT/log.txt"
}

echo 1 > $PARAM
AC=$(cat /sys/class/power_supply/ACAD/online)
MODES="low-power balanced"; [ "$AC" = 1 ] && MODES="$MODES performance" || echo "on battery: skipping performance"
echo "== original mode: $ORIG   AC: $AC   settle: ${SETTLE}s   samples: $SAMPLES" | tee "$OUT/log.txt"
snap "0-start-$ORIG"

for m in $MODES; do
  echo "== switching to $m"
  echo "$m" > $PP || { echo "!! switch to $m failed"; continue; }
  sleep 2; echo "   reads back: $(cat $PP)" | tee -a "$OUT/log.txt"
  snap "$m-t2s"
  sleep "$SETTLE"
  for i in $(seq 1 "$SAMPLES"); do snap "$m-$i"; sleep 10; done
done
echo "== done; dumps in $OUT"
