#!/usr/bin/env python3
"""Minimal UEFI firmware-volume / FFS / section walker (stdlib only).
usage: ffs.py IMAGE OUTDIR  -> writes modules/<guid>_<name>/ sections + index.tsv
"""
import sys, os, struct, uuid, lzma, zlib, re

img = open(sys.argv[1], 'rb').read()
out = sys.argv[2]
os.makedirs(out, exist_ok=True)
index = []
seen_fv = set()

def g(b): return str(uuid.UUID(bytes_le=bytes(b)))

SEC = {0x01:'COMP',0x02:'GUIDED',0x03:'DISP',0x10:'PE32',0x11:'PIC',0x12:'TE',
       0x13:'DXDEP',0x14:'VER',0x15:'UI',0x16:'COMPAT16',0x17:'FVIMG',0x18:'FREEFORM',
       0x19:'RAW',0x1b:'PEIDEP',0x1c:'MMDEP'}
LZMA_GUIDS = {'ee4e5898-3914-4259-9d6e-dc7bd79403cf','d42ae6bd-1352-4bfb-909a-ca72a6eae889'}

def unlzma(d):
    try:
        props, dsz, usz = d[0], struct.unpack('<I', d[1:5])[0], struct.unpack('<Q', d[5:13])[0]
        dec = lzma.LZMADecompressor(lzma.FORMAT_RAW, filters=[{'id': lzma.FILTER_LZMA1,
              'lc': props % 9, 'lp': (props // 9) % 5, 'pb': props // 45, 'dict_size': dsz}])
        return dec.decompress(d[13:], usz)
    except Exception:
        return None

def sections(data, ctx, depth=0):
    """yield (type, payload) recursively, expanding compression/guided/fv"""
    off = 0
    while off + 4 <= len(data):
        sz = int.from_bytes(data[off:off+3], 'little'); t = data[off+3]; hl = 4
        if sz == 0xFFFFFF:
            sz = struct.unpack('<I', data[off+4:off+8])[0]; hl = 8
        if sz < hl or off + sz > len(data): break
        body = data[off+hl:off+sz]
        if t == 0x02:  # guided
            gg = g(body[:16]); doff = struct.unpack('<H', body[16:18])[0] - hl
            payload = body[doff:]
            if gg in LZMA_GUIDS:
                u = unlzma(payload)
                if u: yield from sections(u, ctx, depth+1)
            elif gg == 'fc1bcdb0-7d31-49aa-936a-a4600d9dd083':  # crc32
                yield from sections(payload, ctx, depth+1)
            else:
                yield ('GUIDED:'+gg, payload)
        elif t == 0x01:
            ctype = body[4]
            if ctype == 0: yield from sections(body[5:], ctx, depth+1)
            else:
                u = unlzma(body[5:])  # AMI sometimes uses LZMA here
                if u: yield from sections(u, ctx, depth+1)
                else: yield ('COMP?', body)
        elif t == 0x17:
            walk_fv(body, ctx + '/fv')
            yield ('FVIMG', b'')
        else:
            yield (SEC.get(t, hex(t)), body)
        off = (off + sz + 3) & ~3

def walk_fv(fv, ctx):
    if fv[40:44] != b'_FVH': return
    fvlen = struct.unpack('<Q', fv[32:40])[0]; hlen = struct.unpack('<H', fv[48:50])[0]
    fvguid = g(fv[16:32])
    off = hlen
    # ext header
    exto = struct.unpack('<H', fv[52:54])[0]
    if exto:
        off = exto + struct.unpack('<I', fv[exto+16:exto+20])[0]
    off = (off + 7) & ~7
    fv = fv[:fvlen]
    while off + 24 <= len(fv):
        hdr = fv[off:off+24]
        if hdr == b'\xff' * 24: break
        fg = g(hdr[:16]); ft = hdr[18]; attr = hdr[19]
        sz = int.from_bytes(hdr[20:23], 'little'); hl = 24
        if attr & 0x01:
            sz = struct.unpack('<Q', fv[off+24:off+32])[0]; hl = 32
        if sz < hl: break
        body = fv[off+hl:off+sz]
        name = ''; secs = []
        if ft not in (0x01, 0xf0):  # not RAW / PAD
            for st, p in sections(body, ctx):
                if st == 'UI': name = p.decode('utf-16le', 'ignore').rstrip('\0')
                secs.append((st, p))
        else:
            secs.append(('RAWFILE', body))
        safe = re.sub(r'[^A-Za-z0-9_.-]', '_', name)[:60]
        d = os.path.join(out, f'{fg}_{safe}' if safe else fg)
        k = 0
        while os.path.exists(d): k += 1; d = os.path.join(out, f'{fg}_{safe}_{k}')
        os.makedirs(d)
        for i, (st, p) in enumerate(secs):
            if p: open(os.path.join(d, f'{i:02d}_{st.replace(":", "_")}.bin'), 'wb').write(p)
        index.append((ctx, fvguid, fg, hex(ft), name, ','.join(s for s, _ in secs)))
        off = (off + sz + 7) & ~7

# scan for top-level FVs
for m in re.finditer(b'_FVH', img):
    base = m.start() - 40
    if base < 0 or base in seen_fv: continue
    fvlen = struct.unpack('<Q', img[base+32:base+40])[0]
    if not (0x1000 <= fvlen <= len(img) - base): continue
    seen_fv.add(base)
    walk_fv(img[base:base+fvlen], f'@{base:#x}')

with open(os.path.join(out, 'index.tsv'), 'w') as f:
    for r in index: f.write('\t'.join(r) + '\n')
print(len(index), 'files; FVs at', ', '.join(hex(x) for x in sorted(seen_fv)))
