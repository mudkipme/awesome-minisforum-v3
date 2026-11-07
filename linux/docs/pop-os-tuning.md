# Pop!_OS 24.04 (COSMIC) tuning for the Minisforum V3

What was changed on the test tablet, and why. The goal: under 10 W on battery, as snappy as possible on
the charger, and a low-friction "work laptop" feel for someone coming from Windows.

## Power profiles (`v3-power`)

| | Battery saver | Performance (charger) |
|---|---|---|
| BIOS thermal mode | low-power (15 W ceiling) | performance (28 W ceiling) |
| Power limits (v3-tune, default) | 8 W sustained / 10 W boost | BIOS (28 W), user-adjustable up to 45 W |
| CPU | `powersave` governor, EPP `power`, boost off | `performance` governor, boost on |
| GPU | DPM auto, panel power savings (ABM) 3 | ABM 0 |
| PCIe ASPM | `powersupersave` | `default` |
| Dirty writeback | 60 s | 5 s / 30 s |
| HDA audio power save | 1 s | – |
| Wi-Fi power save | on | off |
| Panel refresh (user `v3-refresh`) | 60 Hz | 165 Hz |
| Background services | the ones marked "Auto" are stopped | restarted |

- `auto` mode follows the charger; `saver` and `performance` can also be forced.
- PCI runtime PM is `auto` in both profiles.
- Re-applied on:
  - charger plug/unplug (udev `ACAD`);
  - resume (system-sleep hook);
  - boot (`v3-power-auto.service`);
  - the driver appearing (udev `platform-profile`).

`power-profiles-daemon` / `com.system76.PowerDaemon` was **masked** so it doesn't fight v3-power.
`thermald` was disabled: it is Intel-only.

## V3 Power panel applet

A COSMIC panel applet ([`applet/`](../applet/), Rust + libcosmic) with four tabs:

- **Power:**
  - Auto / Saver / Performance mode;
  - background services (Docker, Podman, ZeroTier, Bluetooth, CUPS, Avahi, updater, fwupd) with a toggle;
  - an "Auto" checkbox per service, to stop it on battery.
- **Performance:**
  - power limit and fan curve presets plus sliders, separately for battery and charger;
  - live sensors from the driver (fans, eight temperatures, BIOS fan mode).
- **Firewall:** ufw on/off and a rule editor (port, protocol, source, name) through a strictly validating root
  helper (`v3-fw`).
- **Tools:**
  - keep awake (holds a screensaver inhibit; closing the lid still suspends);
  - auto-brightness.

Privileged actions go through systemd units allowed by a polkit rule
([`v3-power/polkit/50-v3-power.rules`](../v3-power/polkit/50-v3-power.rules)). The rule covers only the
allowlisted units, only for the local active user in group `sudo`, and needs no password. The applet never
runs as root.

## Desktop comfort

- **Clipboard history (Win+V):** `cliphist` + `wl-clipboard` + `fuzzel`.
  - COSMIC needs `COSMIC_DATA_CONTROL_ENABLED=1` in `/etc/environment` for clipboard managers to work.
- **Windows-like shortcuts** (COSMIC Settings → Keyboard → Shortcuts → Custom):

  | Shortcut | Action |
  |---|---|
  | Super+E | Files |
  | Super+V | Clipboard |
  | Super+I | Settings |
  | Ctrl+Shift+Esc | System monitor |
  | Super+Shift+S | Screenshot |
  | Super+L | Lock |
  | Super+Up / Super+Down | Maximize / minimize |

- **Keyboard:** repeat delay 250 ms, rate 40/s.
- **Keyring unlock with fingerprint login:**
  - With fingerprint login, `gnome-keyring` stays locked ("no password is available").
  - [`v3-power/v3-keyring-tpm-setup`](../v3-power/v3-keyring-tpm-setup) seals the keyring password with the
    TPM (`systemd-creds --with-key=tpm2`).
  - A user-instance service then unlocks the login keyring at each login, like Windows Hello.
  - The credential is useless on another machine.
- **Auto-brightness:**
  - Ambient light sensor → brightness, applied via cosmic-settings-daemon so the slider and OSD stay in sync.
  - The curve is logarithmic in lux and learns an offset from your manual adjustments.
- **Auto-rotate:** works once the `minisforum_v3_accel` driver binds the accelerometer.
- **Night light:** not possible yet. cosmic-comp has no gamma-control protocol.

## Audio

The ALC245 speakers were very quiet. The kernel quirk `ALC245_FIXUP_BASS_HP_DAC` (SSID `1f4c:e001`) is
active, but **ALSA "Master" sits at −23 dB** after boot.
- PipeWire never touches that control: the sink volume is software-only, which was verified by changing it and
  watching Master.
- `alsactl store` alone doesn't stick: something resets Master during boot, and the shutdown save then stores
  the low value again.

Fix:
- `user/systemd/v3-audio-master.service` sets `amixer -c Generic_1 sset Master 100%` (0 dB) at login, after
  WirePlumber starts.
- The v3-power sleep hook does the same after resume.
- Afterwards, keep the PipeWire sink at 100 % or below.

## Misc

- External USB SSD install: a udev rule marks it non-rotational, with `mq-deadline` and read-ahead 256 KB.
  The bridge passes no TRIM. The rule is not included: it matches one drive's serial.
- Firefox: VA-API hardware video decode prefs in `user.js`.
- Disabled:
  - ModemManager, thermald, ubuntu-fan, apport, ubuntu-advantage, gnome-remote-desktop;
  - cups-browsed, NetworkManager-wait-online;
  - the Docker and Podman services (their sockets stay enabled, so they start on demand).
- `systemd-oomd` enabled.
- Firewall: ufw enabled, with inbound rules only for what's used (e.g. mDNS 5353/udp, VPN, local share apps
  pinned to a fixed port).

## Lessons

- Don't stop NetworkManager to "test" a polkit deny rule. Fingerprint auth (pam_fprintd via cosmic-osd) can
  silently approve `auth_admin`.
- Don't send raw ALIB/SMU calls you haven't seen the firmware send itself (see
  [power-limits.md](power-limits.md)).
