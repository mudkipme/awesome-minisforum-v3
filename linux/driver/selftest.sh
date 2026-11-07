#!/bin/bash
# Minisforum V3 driver self-test. Read-only: it changes nothing.
# Usage: ./driver/selftest.sh   (as user or root; run with the modules loaded)
set -u
PASS=0; FAIL=0; WARN=0
ok()   { echo "  [ OK ] $*"; PASS=$((PASS+1)); }
bad()  { echo "  [FAIL] $*"; FAIL=$((FAIL+1)); }
warn() { echo "  [WARN] $*"; WARN=$((WARN+1)); }
rd()   { cat "$1" 2>/dev/null; }

echo "== Firmware identity"
V=$(rd /sys/class/dmi/id/sys_vendor); P=$(rd /sys/class/dmi/id/product_name); B=$(rd /sys/class/dmi/id/board_name)
BIOS=$(rd /sys/class/dmi/id/bios_version); EC=$(rd /sys/class/dmi/id/ec_firmware_release)
[ "$V/$P/$B" = "Micro Computer (HK) Tech Limited/V3/HPPAC" ] && ok "device: Minisforum V3 (HPPAC)" || bad "device is '$V/$P/$B'"
case "$BIOS" in 1.05|1.06) ok "BIOS $BIOS (verified)";; *) warn "BIOS '$BIOS' not verified: vendor EC commands will stay off";; esac
[ "$EC" = "8.8" ] && ok "EC firmware $EC (verified)" || warn "EC firmware '$EC' not verified: vendor EC commands will stay off"

echo "== Flash protection (fwupd HSI)"
if command -v fwupdmgr >/dev/null; then
  HSI=$(timeout 60 fwupdmgr security --json 2>/dev/null | python3 -I -c '
import json, sys
for a in json.load(sys.stdin)["SecurityAttributes"]: print(a["AppstreamId"], a.get("HsiResult"))' 2>/dev/null)
  hsi() { echo "$HSI" | awk -v k="org.fwupd.hsi.$1" '$1==k{print $2}'; }
  [ "$(hsi Amd.SpiWriteProtection)" = enabled ] && ok "SPI flash write protection enabled" || warn "SPI write protection: '$(hsi Amd.SpiWriteProtection)'"
  [ "$(hsi Amd.RollbackProtection)" = enabled ] && ok "AMD rollback protection enabled (older BIOS can't boot)" || warn "rollback protection: '$(hsi Amd.RollbackProtection)'"
  [ "$(hsi PlatformFused)" = locked ] && ok "AMD platform fused (production-locked)" || warn "platform fused: '$(hsi PlatformFused)'"
  [ "$(hsi PlatformDebugLocked)" = locked ] && ok "platform debug locked" || warn "platform debug: '$(hsi PlatformDebugLocked)'"
  [ "$(hsi Uefi.Pk)" = valid ] && ok "UEFI platform key valid (not a test key)" || warn "UEFI PK: '$(hsi Uefi.Pk)'"
else
  warn "fwupdmgr not installed, skipping"
fi

echo "== Modules"
lsmod | grep -q '^minisforum_v3_wmi' && ok "minisforum_v3_wmi loaded" || bad "minisforum_v3_wmi not loaded"
lsmod | grep -q '^minisforum_v3_accel' && ok "minisforum_v3_accel loaded" || warn "minisforum_v3_accel not loaded"
lsmod | grep -q '^huawei_wmi' && bad "huawei_wmi is loaded (should be blacklisted)" || ok "huawei_wmi not loaded"
G=/sys/bus/wmi/devices/ABBC0F5C-8EA1-11D1-A000-C90629100000
[ "$(basename "$(readlink $G/driver 2>/dev/null)")" = "minisforum-v3-wmi" ] && ok "WMI GUID bound to minisforum-v3-wmi" || bad "WMI GUID not bound to our driver"
[ -r $G/dp_event_count ] && ok "dp_event_count = $(rd $G/dp_event_count)" || bad "dp_event_count missing"

echo "== Sensors (hwmon minisforum_v3)"
H=""; for h in /sys/class/hwmon/hwmon*; do [ "$(rd $h/name)" = minisforum_v3 ] && H=$h; done
if [ -z "$H" ]; then bad "hwmon minisforum_v3 not found"; else
  for i in 1 2; do
    f=$H/fan${i}_input
    if [ -r "$f" ]; then r=$(rd $f)
      if [ -n "$r" ] && [ "$r" -ge 0 ] && [ "$r" -le 9000 ]; then ok "$(rd $H/fan${i}_label): $r RPM"; else bad "fan$i implausible: '$r'"; fi
    else warn "fan$i not present (vendor channel off?)"; fi
  done
  for i in 1 2 3 4 5 6 7 8; do
    f=$H/temp${i}_input; [ -r "$f" ] || continue
    t=$(rd $f); l=$(rd $H/temp${i}_label)
    if [ -n "$t" ] && [ "$t" -ge 10000 ] && [ "$t" -le 105000 ]; then ok "$l: $((t/1000)) °C"; else bad "$l implausible: '$t'"; fi
  done
  # cross-checks: two independent paths to the same sensor
  t1=$(rd $H/temp1_input); t7=$(rd $H/temp7_input); t2=$(rd $H/temp5_input); t8=$(rd $H/temp8_input)
  if [ -n "$t1" ] && [ -n "$t7" ]; then d=$(( (t1-t7) < 0 ? t7-t1 : t1-t7 ))
    [ $d -le 5000 ] && ok "APU die (EC RAM) vs CPU (EC cmd) agree (Δ $((d/1000)) °C)" || bad "APU die vs CPU (EC cmd) differ by $((d/1000)) °C"; fi
  if [ -n "$t2" ] && [ -n "$t8" ]; then d=$(( (t2-t8) < 0 ? t8-t2 : t2-t8 ))
    [ $d -le 5000 ] && ok "System (EC RAM) vs System (EC cmd) agree (Δ $((d/1000)) °C)" || bad "System sensors differ by $((d/1000)) °C"; fi
  K=""; for h in /sys/class/hwmon/hwmon*; do [ "$(rd $h/name)" = k10temp ] && K=$(rd $h/temp1_input); done
  if [ -n "$K" ] && [ -n "$t1" ]; then d=$(( (K-t1) < 0 ? t1-K : K-t1 ))
    [ $d -le 8000 ] && ok "EC APU die vs k10temp agree (Δ $((d/1000)) °C)" || warn "EC APU die vs k10temp differ by $((d/1000)) °C"; fi
fi

echo "== Power profile"
PP=$(rd /sys/firmware/acpi/platform_profile)
case "$PP" in low-power|balanced|performance) ok "platform_profile = $PP";; "") bad "platform_profile not available";; *) warn "platform_profile = '$PP'";; esac
[ "$(rd /sys/module/minisforum_v3_wmi/parameters/allow_profile_set)" = "Y" ] && warn "allow_profile_set=Y (mode writes allowed)" || ok "allow_profile_set=N (read-only)"

echo "== Accelerometer"
A=""; for d in /sys/bus/iio/devices/iio:device*; do [ "$(rd $d/name)" = lsm6ds3tr-c_accel ] && A=$d; done
if [ -n "$A" ]; then
  ok "lsm6ds3tr-c_accel present ($(basename $A))"
  x=$(rd $A/in_accel_x_raw); y=$(rd $A/in_accel_y_raw); z=$(rd $A/in_accel_z_raw); s=$(rd $A/in_accel_scale)
  [ -n "$x$y$z" ] && ok "raw x=$x y=$y z=$z (scale $s)" || bad "cannot read accel raw values"
  [ -n "$(rd $A/mount_matrix)" ] && ok "mount_matrix: $(rd $A/mount_matrix)" || warn "no mount_matrix exposed"
else bad "accelerometer not found"; fi

echo "== Kernel log"
if dmesg >/dev/null 2>&1; then
  E=$(dmesg | grep -E 'minisforum|st_lsm6dsx' | grep -iE 'timed out|fail|error|unexpected|warn' | tail -5)
  [ -z "$E" ] && ok "no driver errors in dmesg" || { bad "driver messages:"; echo "$E" | sed 's/^/         /'; }
else warn "dmesg not readable as this user (run with sudo for this check)"; fi

echo; echo "Result: $PASS ok, $WARN warnings, $FAIL failures"
[ $FAIL -eq 0 ]
