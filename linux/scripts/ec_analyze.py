#!/usr/bin/env python3
"""Find which EC bytes vary in an ec_read.py --log CSV and what they track."""
import sys, csv, statistics as st
rows = list(csv.DictReader(open(sys.argv[1])))
raw = [bytes.fromhex(r['raw']) for r in rows]
die = [int(r['apu_die_C']) for r in rows]
ac = [int(r['ac_online']) for r in rows]
cur = [int(r['bat_current_mA']) for r in rows]
def corr(a, b):
    try: return st.correlation(a, b)
    except Exception: return float('nan')
# AC transitions
tr = [(rows[i]['t'], ac[i-1], ac[i]) for i in range(1, len(ac)) if ac[i] != ac[i-1]]
print('samples', len(rows), rows[0]['t'], '->', rows[-1]['t'], '| AC transitions:', tr)
print('APU die C min/max', min(die), max(die))
print(f"{'off':>4} {'min':>4} {'max':>4} {'#vals':>5} {'r_die':>6} {'r_ac':>6}  values(first..)")
for o in range(256):
    col = [b[o] for b in raw]
    if len(set(col)) < 2: continue
    vals = []
    for v in col:
        if not vals or vals[-1] != v: vals.append(v)
    print(f"{o:#04x} {min(col):4} {max(col):4} {len(set(col)):5} {corr(col, die):6.2f} {corr(col, ac):6.2f}  {' '.join(map(str, vals[:14]))}")
