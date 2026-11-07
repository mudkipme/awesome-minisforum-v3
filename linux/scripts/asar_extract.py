#!/usr/bin/env python3
"""Extract an Electron app.asar (data only, nothing executed). usage: asar_extract.py ASAR OUTDIR"""
import sys, json, struct, os
a = open(sys.argv[1], 'rb').read(); out = sys.argv[2]
hsize = struct.unpack('<I', a[12:16])[0]
hdr = json.loads(a[16:16 + hsize]); base = 8 + struct.unpack('<I', a[4:8])[0]
def walk(node, path):
    for name, n in node.get('files', {}).items():
        p = os.path.join(path, name)
        if os.path.normpath(p).startswith('..') or os.path.isabs(name): continue
        if 'files' in n: walk(n, p)
        elif n.get('unpacked') or 'offset' not in n: continue
        else:
            os.makedirs(os.path.dirname(os.path.join(out, p)) or out, exist_ok=True)
            o = base + int(n['offset']); open(os.path.join(out, p), 'wb').write(a[o:o + n['size']])
walk(hdr, '')
