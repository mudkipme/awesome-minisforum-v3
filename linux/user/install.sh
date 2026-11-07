#!/usr/bin/env bash
# Per-user tools (run as your normal user, not root):
#   v3-refresh        60 Hz on battery saver, 165 Hz otherwise (follows v3-power)
#   v3-autobrightness adaptive brightness from the light sensor, learns your manual changes
#   v3-keepawake      "keep awake" toggle (used by the applet)
#   v3-clipboard      Win+V style clipboard history (cliphist + fuzzel)
# Needs: python3-gi, iio-sensor-proxy, wl-clipboard, cliphist, fuzzel
set -euo pipefail
cd "$(dirname "$0")"
[ "$(id -u)" -ne 0 ] || { echo "run as your normal user"; exit 1; }
B=${DESTDIR:-}$HOME/.local/bin U=${DESTDIR:-}$HOME/.config/systemd/user
install -d "$B" "$U"
for f in bin/*; do install -m755 "$f" "$B/"; echo "  $B/$(basename "$f")"; done
for f in systemd/*; do install -m644 "$f" "$U/"; echo "  $U/$(basename "$f")"; done
[ -e "${DESTDIR:-}$HOME/.config/fuzzel/fuzzel.ini" ] || install -Dm644 config/fuzzel.ini "${DESTDIR:-}$HOME/.config/fuzzel/fuzzel.ini"
if [ -z "${DESTDIR:-}" ]; then
  systemctl --user daemon-reload
  systemctl --user enable --now v3-refresh.path v3-autobrightness.service cliphist.service v3-audio-master.service
  echo "Win+V: add a custom shortcut in COSMIC Settings -> Keyboard -> Shortcuts running $B/v3-clipboard"
  echo "       (the clipboard needs COSMIC_DATA_CONTROL_ENABLED=1 in /etc/environment, then log out/in)"
fi
