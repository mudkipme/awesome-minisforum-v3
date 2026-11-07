#!/usr/bin/env python3
"""Print HII string IDs from the en-US string package of a PE. usage: hii_strings.py PE [id_lo id_hi | text]"""
import sys, struct, re
d = open(sys.argv[1], 'rb').read()
for m in re.finditer(rb'en-US\x00', d):
    o = m.start() - 46          # string package header: len3 type1 hdrsize4 sio4 window32 langname2 lang
    if o < 0 or d[o+3] != 0x04: continue
    plen = int.from_bytes(d[o:o+3], 'little'); sio = struct.unpack('<I', d[o+8:o+12])[0]
    p, sid, end, tab = o + sio, 1, o + plen, {}
    while p < end:
        bt = d[p]; p += 1
        if bt == 0x00: break
        if bt == 0x14:
            e = p
            while d[e:e+2] != b'\0\0': e += 2
            tab[sid] = d[p:e].decode('utf-16le', 'replace'); sid += 1; p = e + 2
        elif bt == 0x10: p += 0                      # EXT? keep simple
        elif bt == 0x21: sid += 1                    # SKIP1
        elif bt == 0x22: sid += struct.unpack('<H', d[p:p+2])[0]; p += 2   # SKIP2
        elif bt == 0x20: sid += d[p]; p += 1         # DUPLICATE? (approx)
        else: break
    args = sys.argv[2:]
    if len(args) == 2:
        lo, hi = int(args[0], 16), int(args[1], 16)
        for k in range(lo, hi + 1): print(hex(k), repr(tab.get(k)))
    elif args:
        for k, v in tab.items():
            if args[0].lower() in v.lower(): print(hex(k), repr(v))
    break
