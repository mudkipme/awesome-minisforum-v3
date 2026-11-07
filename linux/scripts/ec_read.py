#!/usr/bin/env python3
"""Read-only decoder for the Minisforum V3 (HPPAC) EC RAM.

Source: /sys/kernel/debug/ec/ec0/io (needs root + `modprobe ec_sys`, read-only
by default) or an `xxd` dump file.  This script NEVER writes to the EC.

usage:  sudo python3 -I ec_read.py              # live
        python3 -I ec_read.py dump.txt          # from xxd dump
        sudo python3 -I ec_read.py --log 5 out.csv   # log every 5 s
"""
import sys, time, os

LIVE = '/sys/kernel/debug/ec/ec0/io'

def load(src=None):
    if src is None:
        with open(LIVE, 'rb') as f:
            return f.read(256)
    out = bytearray()
    for line in open(src):
        hexpart = line.split(':', 1)[1][:40].split()
        out += bytes.fromhex(''.join(hexpart))
    return bytes(out)

def w(d, o): return d[o] | d[o + 1] << 8

def decode(d):
    """Confirmed fields come from the DSDT ERAM map + matched against sysfs.
    'guess' fields are hypotheses that still need confirming with more dumps."""
    r = {}
    r['ac_online']            = d[0x60] & 1                 # CHSH bit0 (DSDT)
    r['chsh_raw']             = hex(d[0x60])
    r['apu_die_C']            = d[0x09]                     # DIEH (DSDT, = acpitz)
    r['bat_design_mAh']       = w(d, 0x64)                  # DEC
    r['bat_design_mV']        = w(d, 0x66)                  # DEV
    r['bat_full_mAh']         = w(d, 0x68)                  # LFC
    r['bat_warn_mAh']         = w(d, 0x6A)                  # BTP
    r['bat_current_mA']       = w(d, 0x6C)                  # BTC
    r['bat_remaining_mAh']    = w(d, 0x6E)                  # CAP
    r['bat_voltage_mV']       = w(d, 0x70)                  # BTV
    r['bat_cycles']           = w(d, 0x74)                  # BCC
    r['bat_temp_C']           = round(w(d, 0x7A) / 10 - 273.15, 1)  # TMP (deciKelvin)
    r['bat_percent']          = d[0x7C]
    r['bat_health_pct']       = round(100 * r['bat_full_mAh'] / max(r['bat_design_mAh'], 1), 1)
    r['bctr_0xB0']            = d[0xB0]                     # BCTR (DSDT) – charge-start setting?
    # identified by logging 2026-11-07 (load + AC plug/unplug):
    r['temp_0x04_C']          = d[0x04]   # follows APU (r=0.79) – near-SoC sensor
    r['temp_0x07_C']          = d[0x07]   # follows APU (r=0.83) – near-SoC sensor
    r['temp_0x06_C']          = d[0x06]   # slow, partly follows APU – board/IO?
    r['temp_0x05_C']          = d[0x05]   # ~flat ~41 °C – battery/charger area?
    r['temp_0x08_C']          = d[0x08]   # ~flat ~41 °C – battery/charger area?
    r['ac_flag_0x61']         = hex(d[0x61])                # 0x41 on AC, 0x04 on battery
    st = w(d, 0x72)                                          # Smart-Battery BatteryStatus word
    r['bat_status']           = hex(st) + ' ' + ' '.join(n for b, n in (
        (14, 'TERMINATE_CHARGE'), (7, 'INIT'), (6, 'DISCHARGING'), (5, 'FULL')) if st >> b & 1)
    r['guess_fan_table_0x10'] = d[0x10:0x2F].hex(' ')       # static during log – config table, meaning unknown
    r['guess_0x62']           = w(d, 0x62)
    r['guess_0xE4']           = d[0xE4]
    return r

def main(a):
    if a and a[0] == '--log':
        period, path = float(a[1]), a[2]
        keys = None
        with open(path, 'a') as f:
            while True:
                r = decode(load()); r['t'] = time.strftime('%H:%M:%S')
                raw = load().hex()
                if keys is None:
                    keys = list(r); f.write(','.join(keys + ['raw']) + '\n')
                f.write(','.join(str(r[k]).replace(',', ' ') for k in keys) + ',' + raw + '\n')
                f.flush(); time.sleep(period)
    d = load(a[0] if a else None)
    for k, v in decode(d).items():
        print(f'{k:24} {v}')

if __name__ == '__main__':
    main(sys.argv[1:])
