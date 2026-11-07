#!/usr/bin/env python3
"""Diff two EC RAM snapshots, ignoring bytes that always move (temps, battery telemetry, fan table in different modes).
usage: ec_diff.py BEFORE.bin AFTER.bin"""
import sys
a, b = open(sys.argv[1], 'rb').read(), open(sys.argv[2], 'rb').read()
NOISE = set(range(0x04, 0x0A)) | {0x70, 0x71, 0x73, 0x7A, 0x6C, 0x6D, 0x6E, 0x6F, 0x68, 0x69, 0x72, 0x7C}
print('off   before after  note')
for o in range(256):
    if a[o] != b[o]:
        print(f'{o:#04x}  {a[o]:5d} {b[o]:5d}   {"noise (temps/battery telemetry)" if o in NOISE else "<-- candidate"}')
