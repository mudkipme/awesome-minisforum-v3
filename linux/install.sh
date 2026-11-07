#!/usr/bin/env bash
# System install for the Minisforum V3 tools. Run from the repo root:
#   sudo ./install.sh            v3-power backend (profiles, service auto-stop, power limits, fan curve, firewall helper)
#   sudo ./install.sh --driver   also build + install the kernel drivers with DKMS (fans, sensors, thermal mode, accelerometer)
# DESTDIR=/tmp/x ./install.sh   stage into a folder without touching the system (no systemctl/udev/dkms)
# The applet and the per-user tools are separate: see README.md.
set -euo pipefail
cd "$(dirname "$0")"
D=${DESTDIR:-}
live=$([ -z "$D" ] && echo 1 || echo 0)
want_driver=0; [ "${1:-}" = --driver ] && want_driver=1

if [ "$live" = 1 ]; then
  [ "$(id -u)" -eq 0 ] || { echo "run with sudo"; exit 1; }
  [ "$(cat /sys/class/dmi/id/product_name 2>/dev/null)" = V3 ] || { echo "This is not a Minisforum V3 ($(cat /sys/class/dmi/id/product_name 2>/dev/null)). Aborting."; exit 1; }
  user=${SUDO_USER:-}; [ -n "$user" ] && [ "$user" != root ] || { echo "run with sudo from your normal user"; exit 1; }
else
  user=$(id -un)
fi

inst() { install -D -m "$1" "$2" "$D$3"; echo "  $3"; }

echo "== v3-power"
inst 755 v3-power/sbin/v3-power            /usr/local/sbin/v3-power
for f in v3-tune v3-fw v3-keyring-unlock; do inst 755 v3-power/lib/$f /usr/local/lib/v3-power/$f; done
for f in v3-power/systemd/*; do inst 644 "$f" /etc/systemd/system/$(basename "$f"); done
inst 644 v3-power/udev/99-v3-power.rules   /etc/udev/rules.d/99-v3-power.rules
inst 644 v3-power/polkit/50-v3-power.rules /etc/polkit-1/rules.d/50-v3-power.rules
inst 755 v3-power/sleep/v3-power           /usr/lib/systemd/system-sleep/v3-power
for f in services.conf v3-power.conf; do
  [ -e "$D/etc/v3-power/$f" ] && echo "  /etc/v3-power/$f (kept existing)" || inst 644 v3-power/etc/$f /etc/v3-power/$f
done

# user-editable state (the applet writes these; v3-power/v3-tune validate everything)
install -d -m 755 "$D/var/lib/v3-power"
[ -e "$D/var/lib/v3-power/autostop" ] || echo "docker podman" > "$D/var/lib/v3-power/autostop"
[ -e "$D/var/lib/v3-power/tuning.json" ] || cat > "$D/var/lib/v3-power/tuning.json" <<'EOF'
{
  "saver":       {"power": {"stapm": 8, "fast": 10, "slow": 8}, "fan": "factory"},
  "performance": {"power": "factory", "fan": "factory"}
}
EOF
[ "$live" = 1 ] && chown "$user:$user" /var/lib/v3-power/autostop /var/lib/v3-power/tuning.json

if [ "$want_driver" = 1 ]; then
  echo "== drivers (DKMS)"
  inst 644 driver/minisforum-v3-wmi.conf /etc/modprobe.d/minisforum-v3-wmi.conf
  inst 644 driver/minisforum-v3-accel.modules-load.conf /etc/modules-load.d/minisforum-v3-accel.conf
  mkdir -p "$D/etc/modprobe.d"
  printf '%s\n' '# V3 Power: let v3-power switch the BIOS thermal mode (performance on AC = 28 W, low-power on battery)' \
    'options minisforum_v3_wmi allow_profile_set=1' > "$D/etc/modprobe.d/v3-power-profile.conf"; echo "  /etc/modprobe.d/v3-power-profile.conf"
  if [ "$live" = 1 ]; then
    command -v dkms >/dev/null || { echo "install dkms first: sudo apt install dkms"; exit 1; }
    ver=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' driver/dkms.conf)
    rm -rf "/usr/src/minisforum-v3-wmi-$ver"; cp -r driver "/usr/src/minisforum-v3-wmi-$ver"
    dkms add "minisforum-v3-wmi/$ver" || true
    dkms install "minisforum-v3-wmi/$ver"
    modprobe -r huawei_wmi 2>/dev/null || true
    modprobe minisforum_v3_wmi; modprobe minisforum_v3_accel || true
  fi
fi

if [ "$live" = 1 ]; then
  echo "== enabling"
  systemctl daemon-reload
  systemctl enable --now v3-power-auto.service
  udevadm control --reload
  command -v ryzenadj >/dev/null || echo "NOTE: ryzenadj not found in PATH: power limits are skipped (fan curve and BIOS mode still work). See docs/power-limits.md."
  command -v ufw >/dev/null || echo "NOTE: ufw not installed: the firewall tab needs it (sudo apt install ufw)."
  echo "done. Status: v3-power status ; logs: journalctl -t v3-power -t v3-tune"
fi
