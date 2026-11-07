# Minisforum V3 (HPPAC) Linux platform drivers

No BIOS or EC firmware is modified. Everything is a loadable module.

## minisforum_v3_wmi: events, sensors, fan, thermal mode
| Feature | Interface | EC access |
|---|---|---|
| DP-in event (WMI 0xD2 / `_Q55`) | `/sys/bus/wmi/devices/ABBC0F5C-8EA1-11D1-A000-C90629100000/dp_event_count`, uevent `MINISFORUM_EVENT=DP_IN` | none |
| 6 EC RAM temps | hwmon `minisforum_v3` temp1..6 | read-only via kernel ACPI EC |
| CPU / System temp | hwmon temp7 / temp8 | cmd 0xDD 0x20 / 0x21 (read) |
| CPU / System fan RPM | hwmon fan1 / fan2 | cmd 0xD5 0x18-19 / 0x16-17 (read) |
| Thermal mode | `/sys/firmware/acpi/platform_profile` (low-power/balanced/performance) | 0xDE 0x04 read; 0xDE 1/2/3 write **only with `allow_profile_set=1`** |

All commands come from what the BIOS itself (Setup "EC IO Hardware Monitor", OemDxe) or
Minisforum Space sends. There's a hard whitelist in the code. Handshake and ~3 ms pacing match the BIOS.
Performance is refused on battery. The chosen mode is restored after s2idle resume.

**The mode persists.** The BIOS mirrors it in CMOS 0x63 and in Setup → "FAN Mode"
(Quiet/Balance/Performance), so a mode you set survives a reboot, just as it does with
Minisforum Space on Windows. Undo it by setting another mode, or in BIOS Setup.

Options: `enable_ec_cmds=0` (no vendor channel at all), `allow_profile_set=1` (also
writable at runtime in `/sys/module/minisforum_v3_wmi/parameters/`).

## minisforum_v3_accel: accelerometer / gyro (auto-rotate)
The ACPI device SMOCF05 is an ST LSM6DS3TR-C at I2C 0x6A. Mainline st_lsm6dsx doesn't know the ID
(upstream patch "iio: imu: lsm6dsx: Support SMOCF05 ACPI ID" v2, Feb 2026, not merged).
This module re-creates the I2C device as `lsm6ds3tr-c`, so the **stock** st_lsm6dsx binds.
It also passes the firmware mount matrix SLA0 = diag(-1,-1,-1) (`firmware_matrix=0` = identity).
The ST driver checks WHO_AM_I itself and refuses a wrong chip.
If a future kernel includes the upstream patch, this module detects that and does nothing.

## Temporary test (reboot undoes everything except a changed thermal mode)
    sudo rmmod huawei_wmi
    sudo insmod "$PWD"/driver/minisforum_v3_wmi.ko
    sudo insmod "$PWD"/driver/minisforum_v3_accel.ko
    sudo dmesg | tail -8
    sensors 'minisforum_v3-*'
    cat /sys/firmware/acpi/platform_profile
    ls /sys/bus/iio/devices/; cat /sys/bus/iio/devices/iio:device*/name
    monitor-sensor        # from iio-sensor-proxy: rotate the tablet, watch orientation
Checks:
- temp7 should be close to temp1 and k10temp; temp8 (System) equals EC RAM 0x05 (confirmed on hardware 2026-11-07).
- fan1 changes with load. fan2 may read 0 or fail if the V3 has no system fan.
- Rotation: if left/right or normal/bottom-up are swapped, reload with
  `sudo rmmod minisforum_v3_accel; sudo insmod ... firmware_matrix=0`.

Undo: `sudo rmmod minisforum_v3_accel minisforum_v3_wmi && sudo modprobe huawei_wmi`

## Firmware gating
On load the driver checks DMI: BIOS **1.05/1.06** and EC **8.8** are verified. On any other
firmware, only the passive features stay on (WMI event, EC RAM temps). The vendor EC command channel
(fans, mode, EC-cmd temps) stays off unless `force_unknown_fw=1`. So a future BIOS/EC update can't make
the driver send commands to firmware it was never checked against.

## Pre-install check (read-only)
    sudo rmmod minisforum_v3_wmi minisforum_v3_accel 2>/dev/null
    cd "$PWD"/driver && make
    sudo insmod "$PWD"/driver/minisforum_v3_wmi.ko
    sudo insmod "$PWD"/driver/minisforum_v3_accel.ko
    sudo "$PWD"/driver/selftest.sh
The self-test checks firmware identity, flash protections (fwupd HSI), module binding, sensor plausibility
and cross-checks, profile, accelerometer and dmesg. Install only with **0 failures**.

## Flash backup: not available
flashrom 1.3.0 doesn't support AMD Phoenix (FCH SMBus rev 0x71). It finds no flash chip and reads
nothing (tested 2026-11-07, protections unchanged afterwards). **Don't force it with
`laptop=this_is_not_a_laptop`:** the EC shares the flash. The drivers don't need a backup.

## Permanent install
    sudo cp "$PWD"/driver/minisforum-v3-wmi.conf /etc/modprobe.d/
    sudo cp "$PWD"/driver/minisforum-v3-accel.modules-load.conf /etc/modules-load.d/minisforum-v3-accel.conf
    sudo dkms add "$PWD"/driver
    sudo dkms install minisforum-v3-wmi/0.2
    sudo "$PWD"/driver/selftest.sh     # after reboot: should still be 0 failures
Uninstall: `sudo dkms remove minisforum-v3-wmi/0.2 --all && sudo rm /etc/modprobe.d/minisforum-v3-wmi.conf /etc/modules-load.d/minisforum-v3-accel.conf`

## Fan curve
No vendor *command* sets fan duty, but the live fan table in EC RAM (0x10-0x2E) is writable: tested with
`scripts/fan_100_test.py` (both fans to the 9000 RPM reading, then restored byte-identical). Not in the driver
yet. A mode switch (0xDE) reloads the factory table. See CHANGES.md §14.

## Not possible without firmware changes
- AMD PMF "Smart PC" (the firmware never defines \_SB.PMF; OEMPMF APX7 is a stub).
- Changing the EC's code (8051, shares the flash with the BIOS).
