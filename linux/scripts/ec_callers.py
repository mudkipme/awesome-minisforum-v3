#!/usr/bin/env python3
"""For each call to an EC helper in an objdump listing, report the last cl/dl (cmd/arg) loads before it."""
import sys, re
asm, target = sys.argv[1], sys.argv[2]
lines = [l for l in open(asm) if re.match(r'\s+[0-9a-f]+:', l)]
for i, l in enumerate(lines):
    if re.search(rf'call\s+0x{target}\b', l):
        cmd = arg = None
        for k in range(i - 1, max(0, i - 12), -1):
            t = lines[k]
            m = re.search(r'mov\s+\$(0x[0-9a-f]+),%(cl|ecx|dl|edx)\b', t)
            if m:
                reg = m.group(2)
                if reg in ('cl', 'ecx') and cmd is None: cmd = m.group(1)
                if reg in ('dl', 'edx') and arg is None: arg = m.group(1)
            if re.search(r'\bcall\b', t): break
        addr = l.split(':')[0].strip()
        print(f'{addr:>6}  cmd={cmd}  arg={arg}')
