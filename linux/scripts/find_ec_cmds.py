#!/usr/bin/env python3
"""Find EC command bytes (8-bit immediates) loaded within a short window of port 0x6C/0x66 constants."""
import sys, os, re
root = sys.argv[1]
port_pats = {0x6c: [rb'\xb9\x6c\x00\x00\x00', rb'\xba\x6c\x00\x00\x00', rb'\x66\xba\x6c\x00', rb'\xb1\x6c', rb'\xb2\x6c'],
             0x66: [rb'\xb9\x66\x00\x00\x00', rb'\xba\x66\x00\x00\x00', rb'\x66\xba\x66\x00', rb'\xb1\x66']}
imm8 = re.compile(rb'[\xb0-\xb3]([\x80-\xff])|\xba([\x80-\xff])\x00\x00\x00|\xb9([\x80-\xff])\x00\x00\x00|\x41[\xb0\xb8]([\x80-\xff])')
for d in sorted(os.listdir(root)):
    p = os.path.join(root, d)
    if not os.path.isdir(p): continue
    for f in os.listdir(p):
        if 'PE32' not in f: continue
        b = open(os.path.join(p, f), 'rb').read()
        for port, pl in port_pats.items():
            cmds = {}
            for pp in pl:
                for m in re.finditer(re.escape(pp), b):
                    win = b[max(0, m.start() - 24): m.end() + 24]
                    for im in imm8.finditer(win):
                        v = next(g for g in im.groups() if g)[0]
                        cmds[v] = cmds.get(v, 0) + 1
            if cmds:
                top = ' '.join(f'{k:#04x}x{v}' for k, v in sorted(cmds.items()))
                print(f'port {port:#04x}  {d[:55]:55} {top}')
