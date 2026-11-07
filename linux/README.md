# Minisforum V3 on Linux

Drivers, findings and tools for the **Minisforum V3** 3-in-1 tablet (AMD Ryzen 7 8840U "Hawk Point",
board `HPPAC`). The project recovers what Minisforum only exposes on Windows:
- fan speeds, eight EC temperatures and the BIOS thermal mode;
- auto-rotate;
- real fan curves;
- **power limits up to the full 28 W (and beyond) on the charger**, plus single-digit watts on battery.

Nothing here flashes the BIOS or EC. Everything is a loadable module or a userspace tool, and a reboot
returns the EC to its factory state.

> Tested on: BIOS **1.05**, EC **8.8**, Pop!_OS 24.04 (COSMIC), kernel 7.2.15.
> The driver refuses its EC command channel on unverified BIOS/EC versions.

## What works

| Feature | How | Status |
|---|---|---|
| CPU + system fan RPM, 8 temperatures | `minisforum_v3_wmi` hwmon (`sensors`) | ✅ |
| BIOS thermal mode (Quiet / Balanced / Performance) | `minisforum_v3_wmi` → `platform_profile` | ✅ |
| **28 W sustained on AC** (stock Linux: 15 W) | BIOS performance mode | ✅ measured |
| Beyond 28 W (36–40 W, thermal-limited at 90 °C) | performance mode + ryzenadj fast/slow | ✅ measured |
| Lower limits on battery (8 W) | ryzenadj | ✅ measured |
| Fan curve (six duty levels) | EC RAM fan table, `v3-tune` | ✅ |
| Accelerometer / gyro, auto-rotate | `minisforum_v3_accel` (binds stock `st_lsm6dsx`) | ✅ |
| DP-in WMI event | `minisforum_v3_wmi` uevent | ✅ |
| Speakers too quiet | ALSA Master at −23 dB; see tuning doc | ✅ |
| STAPM via ryzenadj / ALIB | firmware ignores it, use the BIOS mode | ❌ ([why](docs/power-limits.md)) |
| AMD PMF / Smart PC | not implemented in this BIOS | ❌ |

## Repository layout

```
driver/      kernel drivers (GPL-2.0, DKMS): minisforum_v3_wmi, minisforum_v3_accel, selftest.sh
v3-power/    power manager: profiles, service auto-stop, power limits + fan curve (v3-tune),
             firewall helper, systemd units, udev + polkit rules, TPM keyring unlock
applet/      COSMIC panel applet "V3 Power" (Rust, libcosmic)
user/        per-user tools: refresh-rate switch, auto-brightness, keep awake, clipboard history
scripts/     research tools: EC RAM reader/logger/diff, fan-table tests, UEFI FFS unpacker, asar extractor
docs/        firmware-analysis.md · power-limits.md · fan-curve.md · pop-os-tuning.md
install.sh   system installer (sudo); user/install.sh for the per-user tools
```

## Install

The driver alone is useful on any distro. v3-power and the applet target COSMIC but only the applet is
COSMIC-specific.

```sh
sudo apt install dkms build-essential linux-headers-$(uname -r) ufw python3-gi
git clone https://github.com/mudkipme/awesome-minisforum-v3 && cd awesome-minisforum-v3/linux

# 1. drivers + v3-power backend
sudo ./install.sh --driver
sudo ./driver/selftest.sh                 # expect 0 failures

# 2. ryzenadj (for power limits; optional): put a ryzenadj binary at /usr/local/bin/ryzenadj
#    e.g. the ryzenadj-manylinux_2_28-x86_64 release from https://github.com/FlyGoat/RyzenAdj

# 3. panel applet (needs a Rust toolchain)
cd applet && cargo build --release
sudo install -Dm755 target/release/cosmic-applet-v3-power /usr/local/bin/
sudo install -Dm644 com.manu.CosmicAppletV3Power.desktop /usr/local/share/applications/
#    then add "V3 Power" in COSMIC Settings -> Desktop -> Panel -> Applets

# 4. per-user tools (as your user)
cd .. && ./user/install.sh
```

Useful commands:
```sh
v3-power status                     # mode, effective profile, AC, battery, watts
sudo v3-power set auto|saver|performance
cat /run/v3-power/tuning-state      # what v3-tune last applied
journalctl -t v3-power -t v3-tune -t v3-fw
```

Uninstall the driver:
```sh
sudo dkms remove minisforum-v3-wmi/0.2 --all
sudo rm /etc/modprobe.d/{minisforum-v3-wmi,v3-power-profile}.conf /etc/modules-load.d/minisforum-v3-accel.conf
```

## Safety

- **No firmware flashing.** SPI write protection stays on. flashrom doesn't support this chipset anyway; don't force it,
  because the EC shares the flash.
- **EC commands:** the driver sends only commands that Minisforum's own BIOS Setup and Windows app send. A hard
  whitelist enforces that.
- **Fan curve writes:** only the six duty bytes are written, after a byte-for-byte check against the factory table.
  The EC reloads its factory table on every mode switch and at boot.
- **Power limits:** clamped to 5–45 W. The chip's own 90 °C limit is never changed.
- **ALIB:** don't call `\_SB.ALIB` function 1 by hand. It hung the SMU in testing
  ([details](docs/power-limits.md)).
- **Windows note:** Minisforum Space installs `WinRing0x64.sys` (CVE-2020-14979).

## Credits and references

- Driver and firmware analysis were done with static analysis of the vendor BIOS and app. Nothing vendor was ever
  executed.
- Same EC family: the mainline `minisforum-um780xtx` hwmon driver (Sebastián Peyrott),
  [ms-a1-fan-controller](https://github.com/Gitii/ms-a1-fan-controller),
  [ms-s1-max-fans-control](https://github.com/raimondomartire/ms-s1-max-fans-control).
- Accelerometer: Samuel Dionne-Riel's "iio: imu: lsm6dsx: Support SMOCF05 ACPI ID" patch.
- ALIB parameter ids: coreboot `amdblocks/alib.h`, [hhd-dev/adjustor](https://github.com/hhd-dev/adjustor).
- [RyzenAdj](https://github.com/FlyGoat/RyzenAdj).

## License

GPL-2.0-only (see `LICENSE`). The kernel drivers carry SPDX headers.
