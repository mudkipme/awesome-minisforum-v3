# Power limits (TDP) on the Minisforum V3

Ryzen 7 8840U (Hawk Point, family 25 model 0x75), BIOS 1.05, EC 8.8, Linux 7.2.15, ryzenadj 0.19.0.
All numbers below were measured on one tablet on 2026-11-07, with a 16-thread busy-loop load.

## TL;DR

- **The BIOS thermal mode sets the sustained power limit (STAPM).** Low-power ("Quiet") = 15 W,
  performance = 28 W. The tablet boots in whatever mode was last set (stored in CMOS), usually low-power,
  so a stock Linux install is stuck at **15 W even on the charger**.
- Switching the mode uses EC command `0xDE 1|2|3`. That is the only thing Minisforum Space does in Windows
  (disassembled `minisforum.dll` → `setThermalMode`); there is no hidden TDP call. On Linux,
  [`driver/`](../driver/) `minisforum_v3_wmi` sends the same command through `platform_profile`
  (needs `allow_profile_set=1`).
- In performance mode, **ryzenadj can raise the fast/slow PPT limits above 28 W** and the chip uses them
  (36–40 W measured, then the 90 °C Tctl limit takes over).
- **STAPM itself cannot be written** (neither raised nor lowered) through ryzenadj or ALIB. Only the
  mode changes it.
- **Lowering works in any mode** via fast/slow/APU-slow limits (8 W measured).

## Measurements

| Setup | Sustained package power | All-core clock | Tctl | Fans |
|---|---|---|---|---|
| BIOS low-power, any ryzenadj limit above 15 W | 15.0 W (pinned) | ~2.3 GHz | ~55 °C | ~3500 rpm |
| BIOS low-power + ryzenadj 8 W slow / 10 W fast | ~8 W | ~1.2 GHz | – | – |
| **BIOS performance, factory limits** | **28 W for 2 min** | ~3.5 GHz | 79 °C | ~6000 rpm |
| BIOS performance + ryzenadj fast 40 W / slow 35 W, fans 100 % | **36–40 W** | 3.8–4.2 GHz | 90 °C (thermal limit) | max |

Limits reported by `ryzenadj -i` right after a mode switch (fresh boot):

| BIOS mode | STAPM | PPT fast | PPT slow | APU slow | Tctl | STT (skin) |
|---|---|---|---|---|---|---|
| low-power (0xB0) | 15 W | 15 W | 15 W | 15 W | 90 °C | 45 °C |
| balanced (0xB1) | 15 W\* | 22 W | 22 W | 22 W | 90 °C | 45 °C |
| performance (0xB2) | **28 W** | 28 W | 28 W | 28 W | 90 °C | 45 °C |

\* Balanced was only read once, during a session in which performance mode *also* reported STAPM 15 W
(see "SMU state" below), so the balanced STAPM value is unconfirmed.

## What does NOT work (so you don't have to try)

| Attempt | Result |
|---|---|
| `ryzenadj --stapm-limit=…` (raise or lower) | "Successfully set", value unchanged |
| `ryzenadj --stapm-time=…` | readback stays 0 (`--slow-time` does change) |
| ALIB DPTC (`\_SB.ALIB 0x0C`) id `0x05` sustained power | accepted (returns 0), ignored |
| ALIB ids `0x01` STAPM time, `0x2E` STT min limit, `0x22` STT skin limit | no effect on the 15 W cap |
| `ryzenadj --apu-skin-temp=60` | readback 60 °C, still 15 W |
| Unloading `amd_pmf` | no change. This BIOS has **no PMF device** (`\_SB.PMF` is never defined) |
| AMD PMF / Smart PC policies | not implemented in this firmware |

ALIB DPTC **does** work for other ids, e.g. `0x06` fast PPT (verified: readback changed). Parameter ids
are those used by coreboot (`amdblocks/alib.h`) and Handheld Daemon's adjustor (which lists the V3).

### ⚠ Don't call ALIB function 1

`\_SB.ALIB 0x01 b030000` (the AC/DC notification the DSDT sends from `_PSR`) **hung the SMU**: amdgpu
reported `SMU: No response`, the GPU reset failed, and the tablet needed a hard reboot. Nothing persistent
was harmed, but don't send it by hand.

### SMU state

During a long session of ryzenadj/ALIB experiments, performance mode reported STAPM 15 W and stayed
pinned at 15 W. After a reboot, the same mode switch gave 28 W immediately. If performance mode seems capped
at 15 W, reboot before drawing conclusions.

## How the tools here use this

- `v3-power` (see [`v3-power/`](../v3-power/)):
  - **on AC**, it sets `platform_profile = performance`, which gives 28 W;
  - **on battery**, it sets `low-power`;
  - it then runs `v3-tune`.
- `v3-tune` applies the user's limits from `/var/lib/v3-power/tuning.json` with ryzenadj:
  - fast, slow and APU-slow limits, clamped to 5–45 W;
  - `"factory"` writes back the BIOS values of the current mode.
- **Charger events:** the firmware re-sends its own limits on every plug/unplug and mode change. v3-power
  re-applies the user's limits 8 s later. The udev rules trigger it on charger events, on resume, and when the
  driver's platform-profile device appears at boot.
- **Thermal safety:** the Tctl limit (90 °C) and the skin-temperature limit are never changed. Above 28 W the
  chip simply throttles at 90 °C, so pair high limits with a stronger fan curve
  (see [fan-curve.md](fan-curve.md)).

## ryzenadj notes

- The prebuilt `ryzenadj-manylinux_2_28-x86_64` 0.19.0 works on this kernel through the `/dev/mem`
  fallback, without `ryzen_smu`. It prints "no compatible ryzen_smu kernel module found".
- It detects "Hawk Point", SMU BIOS interface 14, PM table version 0x4C0008. `PPT VALUE APU` reads `nan`, and
  `StapmTimeConst` reads 0.
- Run it with the working directory set to `/` (or any root-owned directory) when it runs as root.
