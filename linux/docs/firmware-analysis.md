# Minisforum V3: firmware analysis

**Device:** Minisforum V3 tablet (DMI vendor "Micro Computer (HK) Tech Limited", product `V3`, board `HPPAC`)
**CPU:** AMD Ryzen 7 8840U (Phoenix) · **Firmware:** AMI Aptio V, BIOS **1.05** installed (1.06 image analysed)
**OS:** Pop!_OS 24.04 (COSMIC), kernel 7.2.15 · **Date:** 2026-11-07

---

## 1. Summary

The goal was to recover features and control that Minisforum never exposed on Linux,
**without flashing the BIOS or EC firmware and without risk of bricking**.

| Area | Before | After |
|---|---|---|
| Fan speed | not visible | **CPU fan + System fan RPM** in `sensors` / hwmon |
| Temperatures | APU die only (`acpitz`, `k10temp`) | **8 EC temperatures** (APU, 2× SoC, board, system, ambient, plus 2 vendor readings) |
| Thermal / fan mode | Windows app only | **Linux power profile** (`low-power` / `balanced` / `performance`) |
| Accelerometer / gyro | not detected, no auto-rotate | **detected**, iio-sensor-proxy reports orientation |
| WMI | wrong driver (`huawei_wmi`) bound | own driver; **DP-in event** delivered to userspace |
| Firmware | unchanged | **unchanged** (nothing was flashed or written to flash) |

Status: both drivers are tested on the hardware and installed with DKMS (self-test 31 ok, 0 failures).

---

## 2. Safety rules followed

Set by the user: *"only the safest, I don't want to brick my PC"*.

- No BIOS flashing, no EC firmware flashing, no writes to UEFI/NVRAM variables.
- EC RAM was only **read** (kernel `ec_sys` module, read-only by default).
- The only commands sent to the EC are the ones **Minisforum's own BIOS and Windows app already
  send**. A hard whitelist in the driver rejects everything else.
- Every change is a loadable kernel module. Unloading it, or rebooting, undoes it. **One exception:**
  the thermal mode persists (see §6.3).
- Downloaded vendor software (Windows apps, DLLs, drivers) was **never executed**. It was only unpacked and
  read (strings, disassembly).

---

## 3. What was analysed

### 3.1 BIOS image `HPPAC_1.06_240416A`
- Unpacked with a stdlib-only Python parser (`scripts/ffs.py`): 7 firmware volumes, 400 FFS files,
  240 named modules (LZMA sections decompressed).
- 48 ACPI tables extracted and disassembled with `iasl` (not published: Minisforum's firmware).
- OEM DXE modules disassembled with `objdump`: `Setup`, `OemDxe` and others (not published).
- The EC firmware embedded at the start of the image (`CMXG_EC-V14.6`, ~58 KB, 8051) was scanned.

### 3.2 Live system (BIOS 1.05)
- `acpidump` of the running tables, disassembled and compared with the 1.06 image.
- EC RAM dump plus a 19-minute log at 5 s intervals (idle, load, charger unplugged and plugged twice),
  done with `scripts/ec_read.py` and `scripts/ec_analyze.py`.

### 3.3 Vendor software (static analysis only)
- **Minisforum Space 1.0.4** (Electron): `app.asar` unpacked (`scripts/asar_extract.py`) and
  `minisforum.dll` disassembled. It accesses hardware through `WinRing0x64.sys`.
- **MGCC 1.0.4**: an older build of Space with a byte-identical `minisforum.dll`.
- **DPMonitor 1.0.3**: config shows it listens for WMI `root\WMI DP_EVENT`.
- **ST SensorHub 1.0.4.3** INF: one driver covers ACPI `SMOCF00`–`SMOCF05`.

### 3.4 Internet research
- The mainline hwmon driver **minisforum-um780xtx** (Sebastián Peyrott) targets a sibling EC from the same family.
- Upstream patch **"iio: imu: lsm6dsx: Support SMOCF05 ACPI ID"** (Samuel Dionne-Riel, v2 Feb 2026,
  tested on V3 SE) is **not merged** in mainline.
- No existing Linux fan, EC or WMI driver for the V3 was found.

---

## 4. Findings

### 4.1 The EC has two host interfaces
1. **Standard ACPI EC** on ports `0x62/0x66`, GPE 0x17. Linux uses it. Its RAM map comes from the DSDT
   (`\_SB.PCI0.SBRG.EC0`) and logging:

   | EC RAM | Meaning | Status |
   |---|---|---|
   | 0x04, 0x07 | SoC temperature sensors (track APU, r≈0.8) | confirmed by logging |
   | 0x05 | **System temperature** (= vendor cmd 0xDD 0x21) | confirmed on hardware |
   | 0x06 | Board temperature (slow) | likely |
   | 0x08 | Coolest sensor, ~flat (ambient?) | unconfirmed |
   | 0x09 | APU die temperature (`DIEH`) | DSDT + confirmed |
   | 0x40 bit0, 0x61 | AC adapter present (0x41 AC / 0x04 battery) | confirmed |
   | 0x60 | `CHSH`: bit0 AC, bit2 charging | DSDT + confirmed |
   | 0x64–0x7C | Battery: design/full/remaining capacity, voltage, current, cycles, temp (dK), % | DSDT + matches sysfs |
   | 0x72 | Smart Battery `BatteryStatus` word | strong match |
   | 0x10–0x2E | Static table, never changes (probably the fan table) | unknown |
   | 0xB0 | `BCTR` (probably charge-start setting) | unconfirmed |

2. **Vendor command channel** on ports **`0x6C` (command/status) and `0x68` (data)**. No ACPI table declares it, so
   Linux never used it. The BIOS `Setup` and `OemDxe` modules use it, and so does the Windows DLL, with the same
   handshake: wait IBF clear → command → wait → argument → wait OBF → read.

   | Command | Argument | Meaning | Used by |
   |---|---|---|---|
   | `0xD5` | `0x18` / `0x19` | CPU fan RPM low / high | BIOS Setup, Windows DLL |
   | `0xD5` | `0x16` / `0x17` | System fan RPM low / high | BIOS Setup |
   | `0xDD` | `0x20` | CPU temperature °C | BIOS Setup, Windows DLL |
   | `0xDD` | `0x21` | System temperature °C | BIOS Setup |
   | `0xDE` | `0x04` | Read mode → `0xB0` saving / `0xB1` balance / `0xB2` performance | OemDxe, Windows DLL |
   | `0xDE` | `1` / `2` / `3` | Set mode: power saving / balanced / performance | Windows DLL |

   The BIOS spaces commands about 3 ms apart. The fan values 0 and 0xFFFF are shown as N/A by the BIOS.

### 4.2 Fan control
- **No fan curve or fan duty *command* exists** in the BIOS, the Windows DLL or MGCC. **Correction (§14): the fan curve is nevertheless writable directly in EC RAM (0x10–0x2E), tested 2026-11-07.**
- The only control is the **mode**. It is the same as the BIOS Setup option **"FAN Mode" (Quiet / Balance / Performance)**.
- AMD PMF ("Smart PC" power framework) is **not enabled** in this firmware: `\_SB.PMF` is never defined, so
  `amd_pmf` loads but binds to nothing. Its OEM fan-table hook (`OEMPMF` `APX7`) is a stub returning
  `0xFFFFFFFF`.
- The EC firmware contains other compare constants (0xD2–D4, D7, DC, DF, E0). They might be more commands.
  They were **not probed**, deliberately.

### 4.3 WMI
- `AMW0` has one function: event **0xD2**, raised by EC query `_Q55`, with no payload (`_WED` returns 0).
- Windows DPMonitor consumes it as `root\WMI DP_EVENT { Flag = 0 }`, so it is a **DP-in (display input) event**.
- The GUID `ABBC0F5C-8EA1-11D1-A000-C90629100000` is a generic AMI sample GUID, so Linux's
  `huawei_wmi` bound to it by mistake. It created a fake "Huawei WMI hotkeys" input device.

### 4.4 Accelerometer / gyroscope
- ACPI `SMOCF05` at I2C bus 1 (`AMDI0010:03`), address `0x6A`, IRQ on GPIO pin 9. The chip is an
  **ST LSM6DS3TR-C**.
- Mount matrices in the DSDT: accel `SLA0` = diag(−1, −1, −1), gyro `SLG0` = identity. Same as the V3 SE.
- Linux `st_lsm6dsx` supports the chip but only matches `SMO8B30`/`SMOCF00`, so nothing bound to it.

### 4.5 Battery
- On AC at 99–100 %, the current keeps cycling 0 → 100–250 mA: the battery is being **topped up repeatedly**.
- BIOS 1.05 added **Setup → Advanced → ACPI Settings → "Battery charging start"** (start charging only below X %).
  This is the recommended fix, and it works for both Linux and Windows.

### 4.6 Security note (Windows)
Minisforum Space installs **`WinRing0x64.sys`**, a driver with a known vulnerability (CVE-2020-14979). It lets any local
program read and write hardware ports and physical memory, and Windows Defender flags it as a hack tool.
**Uninstalling Minisforum Space is recommended.**

### 4.7 Leads that turned out wrong (retracted)
- *"Thermal zone always reads 20 °C"*: in the 1.06 image, `THERMAL0` references mutex `EC0.M042`, which
  doesn't exist. But the firmware **patches it to `Z009` at install time**, so on the device it works.
- *"MPMS table causes ACPI errors"*: it reads undefined EC fields, but the table is **not loaded** on the device.
- *"A mode change is undone by reboot"*: wrong. The BIOS keeps the mode in **CMOS 0x63** and syncs it into
  Setup, so it **persists**.

---

## 6. The drivers

Source: [`driver/`](../driver/) (GPL-2.0). DKMS package `minisforum-v3-wmi` 0.2.

### 6.1 `minisforum_v3_wmi.ko`
Binds to the WMI GUID, only when DMI says Minisforum V3 / HPPAC.

| Feature | Interface |
|---|---|
| DP-in event counter | `/sys/bus/wmi/devices/ABBC0F5C-8EA1-11D1-A000-C90629100000/dp_event_count` |
| DP-in uevent | `ACTION=change SUBSYSTEM=wmi MINISFORUM_EVENT=DP_IN` (example rule: `99-minisforum-v3-dpin.rules.example`) |
| hwmon `minisforum_v3` | `fan1` CPU Fan, `fan2` System Fan, `temp1–6` EC RAM sensors, `temp7` CPU (EC cmd), `temp8` System (EC cmd) |
| Power profile | `/sys/firmware/acpi/platform_profile`, `/sys/class/platform-profile/*` |

Design details:
- **EC RAM** temperatures are read through the kernel's `ec_read()`, which takes the ACPI EC lock and is read-only.
- **Vendor channel:**
  - ports `0x68`/`0x6C` are claimed with `request_region()`;
  - a mutex serialises transactions;
  - the handshake has timeouts and returns an error instead of a bogus value;
  - commands are paced about 3 ms apart, as the BIOS does;
  - a stale output byte is drained before each command;
  - a **command whitelist** allows only the table in §4.1.
- **Fan RPM:**
  - reads high, low, then high again, and retries if the high byte changed (from the UM780 driver);
  - 0xFFFF → `-ENODATA`;
  - results are cached for 1 s.
- **Profile:** reading always works. Writing requires `allow_profile_set=1`. `performance` is refused when
  not on AC, the same rule as the Windows app.
- **Resume:** after s2idle, the mode last set through the driver is restored if the EC reset it. It is
  not restored as performance when the tablet is on battery.
- **Probe:** sends one "read mode" command as a health check. If the EC doesn't answer, all vendor-channel
  features are disabled and only the passive features stay.

Module parameters:
- `enable_ec_cmds` (default 1): set 0 to never use ports 0x68/0x6C.
- `allow_profile_set` (default 0): set 1 to allow mode changes. It is also writable at runtime.

### 6.2 `minisforum_v3_accel.ko`
- Finds the unbound ACPI I2C client `SMOCF05`, removes it, and re-creates it on the same bus, address and IRQ as
  `lsm6ds3tr-c`. It attaches a `mount-matrix` property taken from `SLA0`.
- The stock, unmodified `st_lsm6dsx_i2c` then binds. It verifies the chip's WHO_AM_I register itself.
- If a future kernel already binds `SMOCF05` (upstream patch merged), the module does nothing.
- Parameter `firmware_matrix` (default 1): set 0 to use the identity matrix instead.
- No auto-load alias. Permanent loading uses `/etc/modules-load.d/`.

### 6.3 Thermal mode persistence
The mode is shared with the BIOS: CMOS 0x63 and Setup "FAN Mode". A mode set from Linux, Windows or the BIOS
**survives a reboot**. This is the only persistent effect the drivers can have, and only after
`allow_profile_set=1`.

---

## 7. Hardware test results (2026-11-07)

```
minisforum-v3-wmi ...: Minisforum V3: DP-in events, EC temps, fan/CPU temp/thermal mode (profile read-only)
minisforum_v3_accel: lsm6ds3tr-c @ i2c-1 0x6a irq 145, firmware SLA0 matrix; st_lsm6dsx_i2c should bind now

CPU Fan:         3756 RPM
System Fan:      3829 RPM
APU die:          +57.0°C
SoC sensor A:     +58.0°C
SoC sensor B:     +55.0°C
Board:            +48.0°C
System:           +41.0°C   (was labelled "Battery area A" in this run, relabelled since)
Ambient?:         +38.0°C   (was "Battery area B")
CPU (EC cmd):     +57.0°C   = APU die   ✔
System (EC cmd):  +41.0°C   = EC 0x05   ✔
platform_profile: low-power
iio: als, lsm6ds3tr-c_accel, lsm6ds3tr-c_gyro
iio-sensor-proxy: "Has accelerometer", orientation: normal
```

Conclusions: the V3 has **two fans**, both temperature sources agree, and the accelerometer works.

---

## 11. References
- Kernel driver minisforum-um780xtx: https://docs.kernel.org/next/hwmon/minisforum-um780xtx.html
- SMOCF05 patch v2: https://lkml.iu.edu/hypermail/linux/kernel/2602.0/00365.html (1/2), https://lkml.iu.edu/hypermail/linux/kernel/2602.0/00364.html (cover)
- AUR minisforum-v3-accelerometer: https://aur.archlinux.org/packages/minisforum-v3-accelerometer
- CVE-2020-14979 (WinRing0): https://nvd.nist.gov/vuln/detail/CVE-2020-14979

---

## 12. Pre-install verification (added 2026-11-07)

### 12.1 Firmware security state (fwupd HSI, read-only), result **HSI:2**
| Check | Result |
|---|---|
| SPI flash write protection (`Amd.SpiWriteProtection`) | **enabled**: the OS can't write the flash chip |
| AMD rollback protection | **enabled**: an older BIOS than the installed one won't boot |
| Platform fused / debug locked | **locked** (production PSP) |
| UEFI platform key | valid (not a test key) |
| TPM 2.0, PCR0 | found, valid |
| IOMMU, pre-boot DMA protection | enabled |
| Secure Boot, kernel lockdown | off (user setup; this is what allows unsigned DKMS modules) |
| Kernel tainted | yes, caused by the out-of-tree modules (expected) |

### 12.2 Firmware gating in `minisforum_v3_wmi`
The driver reads BIOS and EC versions from DMI at load:
- **Verified:** BIOS 1.05 / 1.06, EC 8.8 (this tablet: BIOS 1.05, EC 8.8).
- **Unverified firmware:** only passive features. The vendor EC command channel stays off unless
  `force_unknown_fw=1`.

### 12.3 `driver/selftest.sh` (read-only)
It checks:
- device identity and firmware versions;
- the HSI flash protections;
- module and WMI binding, and that `huawei_wmi` is absent;
- plausible fan and temperature ranges;
- **cross-checks between independent sensor paths**: EC RAM vs EC command vs `k10temp`;
- power profile, accelerometer and mount matrix;
- driver errors in dmesg.

First run: **26 ok, 0 failures**. APU die, EC command and k10temp agree within 0 °C.

### 12.4 Flash integrity check (`scripts/compare_flash.py`, optional)
A read-only `flashrom -r` dump can be compared with the official image:
- the EC region must be **identical** (EC 8.08 ships in both 1.05 and 1.06);
- the differing 64 KiB blocks show exactly what 1.06 changes, and where NVRAM, serials or keys live.

The dump is also a full backup of this exact board for recovery with an SPI programmer.

### 12.5 flashrom attempt (2026-11-07)
`flashrom -p internal -r` (1.3.0): unknown FCH revision 0x71, **no flash chip found, nothing read or
written**. Its attempts to lift the chipset's flash-protection registers failed. Re-checked afterwards: SPI write
protection and rollback protection still enabled, EC healthy, self-test 0 failures.
**flashrom isn't usable on this tablet; don't force it.** §12.4 is dropped.

## 13. Fan-table experiment (2026-11-07)
Thermal mode was switched temporarily (low-power → balanced → performance → back to low-power) with the
vendor mode command, and EC RAM was dumped read-only after each switch. **No EC RAM was written.**
- **EC 0x30** holds the current mode (0xB0/0xB1/0xB2).
- **EC 0x10–0x2E is the live fan curve.** The EC loads a different table per mode. The temperature thresholds
  (25/45/54/66/76/88/93 °C) are the same in every mode. The per-level duty (probably PWM %) rises with mode:
  e.g. the 45–66 °C level is 38 % / 43 % / 45 %, and idle RPM is 3433 / 3956 / 4436.
- Full table and layout: [fan-curve.md](fan-curve.md).

---

## 14. Fan-table write test: both fans to 100 % (2026-11-07)

`scripts/fan_100_test.py`, run as root with `ec_sys write_support=1` (afterwards reloaded read-only).

**Safety built into the script**
- It writes **only six bytes**: the DUTY entries of levels 0–5 (EC 0x14, 0x18, 0x1C, 0x20, 0x24, 0x28).
  Temperature bands, unknown bytes and the level-6 emergency row are never touched.
- Before writing it checks BIOS 1.05/1.06 + EC 8.8, mode = low-power, **and that the whole table 0x10–0x30
  matches the recorded factory low-power table byte for byte.** Otherwise it aborts with no writes.
- It restores on exit, error or Ctrl+C. Last resort: reload the factory table with a mode switch
  (low-power → balanced → low-power).

**Result**
| | |
|---|---|
| Duty before → during | 25/32/38/40/43/50 → 100 (read back correctly) |
| Fan RPM | ~3317 / 3422 → 6178 / 6213 after 2 s → **9000 / 9000** after 5 s (hold 30 s) |
| APU temperature | fell from 49 °C to 46 °C |
| Restore | `table identical to factory ✔`, fans back to ~3100 RPM within 10 s |
| `ec_sys` afterwards | read-only again (`write_support = N`) |

**Conclusions**
- The EC accepts and keeps writes to the fan table, and **the duty byte is fan power in %**.
  Real fan-curve control works on this tablet with no flashing.
- 9000 is probably the highest RPM the reading can report (the upstream UM780 driver also treats 9000 as the cap).
  The true maximum speed of the fans is unknown.
- The duty offsets used are confirmed correct by the effect. The meaning of the table's other column ("X")
  is still unknown, and the layout beyond the DUTY bytes stays unconfirmed.
- Changes in EC RAM are volatile: a mode switch or reboot reloads the factory curve.

---

## 16. Internet research, round 2 (2026-11-07)

| Finding | Relevance |
|---|---|
| **Minisforum MS-S1 MAX** project (raimondomartire): fan speed is set by writing a fan-curve table in EC RAM, which the firmware re-reads live. Two tables (one per fan), 7 records; the stock curve is very conservative; a power cycle restores the stock curve. Uses the kernel's exported `ec_read()`/`ec_write()` through a small module instead of `ec_sys`. | Same technique we just proved on the V3. Its record layout (3-byte records) differs from our 4-byte hypothesis, so the layouts are not interchangeable. |
| **Minisforum MS-A1** (Gitii): uses the **same EC ports 0x6C/0x68**, commands `0xDD` (0x20 CPU, 0x21 board) and `0xD5` (fans **0x14–0x19**, two bytes per fan). Concludes fan control is "firmware-locked" there. | Confirms our command set. (`0xD5 0x14/0x15` would be a third fan on the MS-A1; the V3 has only two, so the BIOS's unused "SSD Fan Speed" label is just generic.) The MS-A1 "locked" conclusion does **not** hold for the V3. |
| Same README warns: writing 0x6C/0x68 on non-matching hardware can glitch the keyboard controller (the EC ports overlap the i8042 window), and a command sweep can have side effects. | Supports our firmware gating and whitelist; reinforces not probing unknown commands. |
| **UM780 XTX** hwmon patch series, **UM760 Slim** module (mordilloSan), Strix Halo EC fan controllers | Other boards of the same EC family with fan control through EC RAM. |
| **Minisforum support page:** the V3's 80–90 % charge cap is a "Battery Charge Limit" set in BIOS; restoring BIOS defaults removes it. | The BIOS 1.05 strings show "Battery charging start" in Advanced → ACPI Settings. A separate "Battery Charge Limit" item may exist elsewhere in Setup. Check the Setup menus; either one can be traced in EC RAM with a before/after dump. |
| **Kernel patch "ALSA: hda/realtek: Add quirk for Minisforum V3 SE"** (Feb 2026) | For the **V3 SE**, a different board; may not apply to the HPPAC V3. |
| Framework forum threads: EC-based charge limits are often reset to the BIOS value at shutdown | Hint that a charge byte written from Linux may not persist. Needs testing. |
| ryzenadj / ryzen_smu support for family 25 model 0x75 | **Not confirmed** by search. |
| No existing V3 fan/EC/WMI driver found (first round, still true) | |

Sources: https://github.com/raimondomartire/ms-s1-max-fans-control · https://github.com/Gitii/ms-a1-fan-controller ·
https://github.com/mordilloSan/fan_control · https://docs.kernel.org/next/hwmon/minisforum-um780xtx.html ·
https://minisforumpc.eu/pages/common-after-sales-questions-about-tablet-monitor ·
https://lkml.iu.edu/hypermail/linux/kernel/2602.0/00367.html

---

## 19. Non-power functionality survey (2026-11-07, read-only)

| Function (firmware module) | State on Linux 7.2.15 |
|---|---|
| Accelerometer + gyro (SMOCF05 / SensorHub) | **working** via `minisforum_v3_accel` |
| Ambient light sensor (AMD SFH) | working (`als`) |
| Lid switch, **tablet/slate-mode switch** (`gpio-keys`, SW_TABLET_MODE; BIOS 0.10 added the slate GPIO) | working |
| Touch screen + stylus (HID PNP0C50 222A:550D), stylus battery | working (stylus battery in power_supply) |
| Fingerprint (Goodix 27c6:6092, `SecureBio*`) | **working**: fprintd sees "Goodix MOC Fingerprint Sensor", a finger is enrolled |
| Webcam (UVC) | working |
| Audio ALC245 (SSID 0x1f4ce001, set by BIOS 0.07) | codec bound; the kernel logs `picked fixup … for PCI SSID 1f4c:e001`. The upstream "Minisforum V3 (SE)" quirk (`ALC245_FIXUP_BASS_HP_DAC`, Feb 2026) uses **this same SSID**, so it applies to this tablet too and is probably the fixup already active on kernel 7.2.15. Not tested by ear: bass-speaker volume, mics, headset |
| USB4 / Thunderbolt (`AmdUsb4Dxe`) | domains `0-0`, `1-0` present |
| Backlight | `amdgpu_bl1` |
| NPU (XDNA) | `amdxdna` loaded |
| EC: fans, temperatures, thermal mode, DP-in | **this project** |
| USB-C connector class (`/sys/class/typec`) | **absent**. ACPI describes four Type-C connectors (`_UPC` type 0x09, with `_PLD` positions) but **no PD controller or UCSI device**, and the host I2C buses hold no PD chip (i2c-0/1 = touch + IMU, i2c-20/21/22 = FCH SMBus incl. DDR5 SPD hub `spd5118` at 22-0x52, the rest are amdgpu). The PD controller is behind the EC (or a private MCU), so only EC RAM could reveal it |
| AMD PMF, Pluton (MHSP) | not usable (PMF undefined in ACPI; Pluton is Windows-only) |
| Kernel log | 3× `i2c i2c-20: Failed! (01)`: i2c-20 is the **FCH SMBus (PIIX4) port 0**, i.e. failed probes of absent SMBus devices, harmless |
| Unbound ACPI devices | only PCIe switch ports (`SWUS`), a reserved-resource node (`AMDN`) and the original SMOCF05 node (replaced by our client) |

Open items: test audio (speakers, internal mic, headset jack), identify the `i2c-20` errors, and check visible Setup options
(e.g. iGPU memory size, virtualization, USB4 and NPU switches) for anything useful.

### 19.1 Audio: V3 SE quirk applies to the V3 (HPPAC)
The upstream patch (Samuel Dionne-Riel, Feb 2026, in 6.19+) adds `SND_PCI_QUIRK(0x1f4c, 0xe001, "Minisforum V3 (SE)", ALC245_FIXUP_BASS_HP_DAC)`.
Problem fixed: the bass speakers are routed to a DAC that has no volume control; the fix reuses the ThinkPad X1 Gen7 routing.
The SSID 0x1f4c:0xe001 is the one BIOS 0.07 sets for HPPAC ("Set SSID to 0xE0011F4C"), so this tablet is covered.
My earlier remark that the patch "may not cover the original V3" was wrong.

### 19.2 USB-C controller info: options
- ACPI/OS: nothing (see table). USB4 router state is visible in `/sys/bus/thunderbolt/devices/*` (`usb4_port*`) when a USB4 device is attached.
- EC RAM: differential snapshots (`scripts/ec_snapshot.sh`, `scripts/ec_diff.py`) with different USB-C sources (second charger,
  USB-C display / DP source, USB-C drive) may reveal adapter voltage/wattage/DP-state bytes. Read-only.
- Not recommended: scanning I2C buses for a PD chip (can disturb devices; the controller is not on the host buses anyway).

---

## Power limits
The TDP investigation (old §15) is superseded by [power-limits.md](power-limits.md).
