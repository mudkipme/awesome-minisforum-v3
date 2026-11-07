#!/usr/bin/env python3
"""List extracted modules whose code touches I/O ports 0x68/0x6C or 0x62/0x66 (byte patterns)."""
import sys, os, re
root = sys.argv[1]
pats = {
  'out 6C': rb'\xe6\x6c', 'in 6C': rb'\xe4\x6c', 'out 68': rb'\xe6\x68', 'in 68': rb'\xe4\x68',
  'dx=6C': rb'\x66\xba\x6c\x00', 'dx=68': rb'\x66\xba\x68\x00',
  'ecx=6C': rb'\xb9\x6c\x00\x00\x00', 'ecx=68': rb'\xb9\x68\x00\x00\x00',
  'dx=66': rb'\x66\xba\x66\x00', 'dx=62': rb'\x66\xba\x62\x00',
}
for d in sorted(os.listdir(root)):
    p = os.path.join(root, d)
    if not os.path.isdir(p): continue
    for f in os.listdir(p):
        if 'PE32' not in f: continue
        b = open(os.path.join(p, f), 'rb').read()
        hits = {k: len(re.findall(v, b)) for k, v in pats.items()}
        hits = {k: v for k, v in hits.items() if v}
        if any(k.endswith(('6C', '68')) for k in hits):
            print(f'{d[:60]:60} {hits}')
