# Fan curve control on the Minisforum V3

No vendor command sets fan speed, but the EC keeps its **live fan table in EC RAM 0x10–0x30**, re-reads it
continuously, and accepts writes to it. The table is volatile: the EC reloads its factory table on every
thermal-mode switch and at boot.

Tested on BIOS 1.05 / EC 8.8 (`/sys/class/dmi/id/ec_firmware_release` = `8.8`).

## Layout

```
0x10-0x11   header (0x0f 0x19)
0x12 + 4n   row n = [T_down, X, DUTY, T_up]      n = 0..6
0x2e-0x2f   tail
0x30        mode byte: 0xB0 low-power, 0xB1 balanced, 0xB2 performance
```

- **DUTY** is fan power in percent. This is confirmed: writing 100 drives both fans to the 9000 rpm reading.
- **T_up / T_down** are the band edges in °C. They are identical in all three modes.
- **X** varies per mode, non-monotonically. Its meaning is unknown and it is never written.

| Level | Band (°C) | DUTY offset | Low-power | Balanced | Performance |
|---|---|---|---|---|---|
| 0 | < 45 | 0x14 | 25 | 25 | 25 |
| 1 | 45–54 | 0x18 | 32 | 32 | 36 |
| 2 | 54–66 | 0x1C | 38 | 43 | 45 |
| 3 | 66–76 | 0x20 | 40 | 54 | 56 |
| 4 | 76–88 | 0x24 | 43 | 58 | 60 |
| 5 | 88–93 | 0x28 | 50 | 60 | 64 |
| 6 | > 93 (emergency) | 0x2C | 90 | 90 | 90 |

Factory tables (EC 0x10..0x30):
```
low-power   0f190028192d192320362d4226423610284c421e2b584c3a325d58c85a645d00b0
balanced    0f190028192d192320362d7a2b42365b364c42283a584c103c5d58c85a645d00b1
performance 0f190028192d193724362d642d42365b384c42283c584c21405d58c85a645d00b2
```

## Safety rules used by `v3-tune`

1. **Firmware check:** BIOS 1.05/1.06 and EC 8.8 only.
2. **Table check:** every non-duty byte must match the factory table of the current mode byte for byte.
   Otherwise it doesn't write anything.
3. **Writes:** only the six DUTY bytes of levels 0–5. The level 6 emergency row is never touched.
4. **Clamping:**
   - each level is held between the factory **low-power** duty and 100 %;
   - the curve is forced non-decreasing.
5. **Write access:** `ec_sys` is switched to write mode only for the write, then back to its previous
   state (read-only or unloaded).
6. **Verification:** each write is read back and compared.
7. **Re-apply:** the curve is re-applied after every mode switch, charger event and resume, because the EC
   reloads the factory table then.

Reference script with the same checks plus automatic restore: [`scripts/fan_100_test.py`](../scripts/fan_100_test.py).

## Presets in the V3 Power applet

| Preset | Duty levels 0–5 |
|---|---|
| Factory | whatever the current BIOS mode loads |
| Quiet | 25 32 38 40 43 50 (= BIOS low-power) |
| Balanced | 25 32 43 54 58 60 (= BIOS balanced) |
| Cool | 25 36 45 56 60 64 (= BIOS performance) |
| Max | 50 60 72 85 100 100 |
| Custom | one slider per level |

Measured at idle on AC:
- **Max:** 3,500 → 6,300 rpm.
- **All six levels at 100 %:** 9,000 rpm on both fans. That is probably the ceiling of the RPM reading, not
  the real maximum.

## Doing it by hand

```sh
sudo modprobe -r ec_sys && sudo modprobe ec_sys write_support=1
sudo xxd -s 0x10 -l 0x21 /sys/kernel/debug/ec/ec0/io      # check it matches a factory table
# ... write DUTY bytes only ...
sudo modprobe -r ec_sys && sudo modprobe ec_sys          # back to read-only
```
To get the factory curve back, switch the thermal mode away and back, or reboot.
