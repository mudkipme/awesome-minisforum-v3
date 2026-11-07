#!/usr/bin/env python3
"""Compare a flash dump of the V3 with the official HPPAC image. Read-only.

usage: compare_flash.py DUMP.bin [OFFICIAL.bin]
  - checks the EC firmware region is byte-identical (BIOS 1.05 and 1.06 both ship EC 8.08)
  - lists differing 64 KiB blocks, labelled with the region they fall in
"""
import sys, hashlib, re, struct

dump = open(sys.argv[1], 'rb').read()
off_path = sys.argv[2] if len(sys.argv) > 2 else 'HPPAC.BIN'  # official BIOS image from Minisforum
ref = open(off_path, 'rb').read()
print(f'dump     {sys.argv[1]}: {len(dump):#x} bytes, sha256 {hashlib.sha256(dump).hexdigest()[:16]}…')
print(f'official {off_path}: {len(ref):#x} bytes, sha256 {hashlib.sha256(ref).hexdigest()[:16]}…')
if len(dump) != len(ref):
    sys.exit('size mismatch: not a full dump of the same flash size')

# region map from the official image
regions = []
ec_end = next(m.start() for m in re.finditer(rb'\xff{4096}', ref) if m.start() > 0x1000)
regions.append((0, ec_end, 'EC firmware (' + ref[0x50:0x5d].decode('ascii', 'replace') + ')'))
for m in re.finditer(b'_FVH', ref):
    b = m.start() - 40
    if b < 0: continue
    ln = struct.unpack('<Q', ref[b + 32:b + 40])[0]
    if 0x1000 <= ln <= len(ref) - b:
        regions.append((b, b + ln, f'UEFI firmware volume @{b:#x}'))
for m in re.finditer(rb'\$PSP', ref):
    regions.append((m.start(), m.start() + 0x1000, 'AMD PSP directory'))

def label(o):
    hits = [n for s, e, n in regions if s <= o < e]
    return hits[-1] if hits else 'other (PSP blobs / NVRAM / padding)'

ec_same = dump[:ec_end] == ref[:ec_end]
print(f'\nEC firmware region 0..{ec_end:#x}: ' + ('IDENTICAL ✔' if ec_same else 'DIFFERENT ✘'))
if not ec_same:
    print('  dump EC version string:', dump[0x50:0x60])

blk = 0x10000
diff = [o for o in range(0, len(ref), blk) if dump[o:o + blk] != ref[o:o + blk]]
same = len(ref) // blk - len(diff)
print(f'\n64 KiB blocks: {same} identical, {len(diff)} different')
last = None
for o in diff:
    lab = label(o)
    if lab != last:
        print(f'  {o:#09x}  {lab}')
        last = lab
