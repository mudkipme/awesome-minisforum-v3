#!/usr/bin/env python3
"""Minisforum V3: hold both fans at 100 % duty for 30 s by editing the live fan table, then restore it.

Run as root, after loading ec_sys with write support:
  sudo modprobe -r ec_sys; sudo modprobe ec_sys write_support=1
  sudo python3 -I ./scripts/fan_100_test.py
  sudo modprobe -r ec_sys; sudo modprobe ec_sys          # back to read-only

Writes ONLY the six DUTY bytes below, and only if the table matches the recorded
factory low-power table exactly. Restores on exit, error or Ctrl+C.
"""
import glob, os, sys, time

EC = '/sys/kernel/debug/ec/ec0/io'
DUTY_OFFS = [0x14, 0x18, 0x1C, 0x20, 0x24, 0x28]          # levels 0..5 DUTY column
TARGET = 100
HOLD_S = 30
# factory low-power table 0x10..0x30 (recorded 2026-11-07, live-dsl/modes/low-power-3.bin)
FACTORY_LOW = bytes.fromhex('0f190028192d192320362d4226423610284c421e2b584c3a325d58c85a645d00b0')
PP = '/sys/firmware/acpi/platform_profile'
PARAM = '/sys/module/minisforum_v3_wmi/parameters/allow_profile_set'

def rd(path):
    try: return open(path).read().strip()
    except OSError: return ''

def ec_read_all():
    with open(EC, 'rb') as f:
        return f.read(256)

def ec_write(off, val):
    assert off in DUTY_OFFS and 0 <= val <= 100, (off, val)  # hard whitelist
    with open(EC, 'r+b', buffering=0) as f:
        f.seek(off); f.write(bytes([val]))

hw = next((h for h in glob.glob('/sys/class/hwmon/hwmon*') if rd(h + '/name') == 'minisforum_v3'), None)
def fans():
    if not hw: return 'fans: n/a'
    return f"fan1={rd(hw + '/fan1_input'):>5} fan2={rd(hw + '/fan2_input'):>5} apu={int(rd(hw + '/temp1_input') or 0)//1000}°C"

def mode_reload():
    """Last resort: force the EC to reload its factory table via the vendor mode command."""
    print('!! reloading factory table via mode switch low-power -> balanced -> low-power')
    old = rd(PARAM)
    open(PARAM, 'w').write('1')
    for m in ('balanced', 'low-power'):
        open(PP, 'w').write(m); time.sleep(3)
    if old != 'Y': open(PARAM, 'w').write('0')

def main():
    if os.geteuid() != 0: sys.exit('run as root')
    if rd('/sys/class/dmi/id/bios_version') not in ('1.05', '1.06') or rd('/sys/class/dmi/id/ec_firmware_release') != '8.8':
        sys.exit('unverified BIOS/EC firmware, aborting')
    if not os.path.exists(EC): sys.exit('ec_sys not loaded')
    if rd('/sys/module/ec_sys/parameters/write_support') != 'Y':
        sys.exit('load ec_sys with write_support=1 first (see header)')
    cur = ec_read_all()[0x10:0x31]
    if cur != FACTORY_LOW:
        sys.exit('fan table does not match the recorded factory low-power table, aborting (no writes).\n'
                 f'  now:      {cur.hex()}\n  expected: {FACTORY_LOW.hex()}')

    orig = {o: ec_read_all()[o] for o in DUTY_OFFS}
    print('original duty:', {hex(o): v for o, v in orig.items()}, '|', fans())
    restored = False
    try:
        for o in DUTY_OFFS:
            ec_write(o, TARGET)
        back = ec_read_all()
        print('read-back    :', {hex(o): back[o] for o in DUTY_OFFS})
        if any(back[o] != TARGET for o in DUTY_OFFS):
            print('!! EC did not keep the values (it may refresh the table)')
        t0 = time.time()
        while time.time() - t0 < HOLD_S:
            b = ec_read_all()
            print(f'  t={time.time() - t0:4.0f}s  {fans()}  duty@0x1c={b[0x1c]}')
            time.sleep(2)
    finally:
        for o, v in orig.items():
            ec_write(o, v)
        time.sleep(0.5)
        restored = ec_read_all()[0x10:0x31] == FACTORY_LOW
        print('restore      :', 'table identical to factory ✔' if restored else 'MISMATCH')
        if not restored:
            mode_reload()
            print('after reload :', 'factory ✔' if ec_read_all()[0x10:0x31] == FACTORY_LOW else ec_read_all()[0x10:0x31].hex())
    for _ in range(5):
        time.sleep(2); print('  after       ', fans())

if __name__ == '__main__':
    main()
