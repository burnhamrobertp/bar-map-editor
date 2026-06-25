#!/usr/bin/env python3
"""Reverse-engineer DEVICEWORLD > Links. Build a COMPLETE device-id set first
(proper-format ids from the model + inline-format ids scanned from the device's
own Contents `WM\\x02id` block), then score record-format hypotheses by exact
id membership -- a correct format lands ~100%, noise lands ~few%."""
import sys
import struct
from collections import Counter
import tmd_model as M

ID_BLOCK = b"WM\x02id"  # WM block: "WM" + namelen(2) + "id"; payload i32 at +13


def inline_id(contents_raw):
    p = contents_raw.find(ID_BLOCK)
    if p < 0 or p + 13 + 4 > len(contents_raw):
        return None
    return struct.unpack_from("<i", contents_raw, p + 13)[0]


path = sys.argv[1]
world, devs = M.load(path)
for d in devs:
    if d.iid is None:
        d.iid = inline_id(d.contents)
byid = {d.iid: d for d in devs if d.iid is not None}
idset = set(byid)
still_none = [d for d in devs if d.iid is None]
print(f"{len(devs)} devices, {len(idset)} ids resolved, {len(still_none)} still missing")
print("  missing cc:", Counter(d.cc for d in still_none).most_common(8))

raw = world.child("Links").raw
n = len(raw)
U = lambda o: struct.unpack_from("<I", raw, o)[0]
vid = lambda v: v in idset


def try_flat(start, stride, idfields, portfields):
    off, recs, good = start, 0, 0
    while off + stride <= n:
        ok = all(vid(U(off + f)) for f in idfields) and all(raw[off + f] < 32 for f in portfields)
        recs += 1
        good += ok
        off += stride
    return recs, good, n - off


print("\n== flat hypotheses: valid/total (leftover) ==")
TEMPLATES = [
    ("5B  id,port", 5, [0], [4]),
    ("5B  port,id", 5, [1], [0]),
    ("6B  id,port,pad", 6, [0], [4]),
    ("8B  src,dst", 8, [0, 4], []),
    ("9B  src,dst,port", 9, [0, 4], [8]),
    ("10B src,sp,dst,dp", 10, [0, 5], [4, 9]),
    ("12B src,sp,dst,dp,pad", 12, [0, 6], []),
]
best = None
for name, stride, idf, pf in TEMPLATES:
    for start in range(0, 6):
        r, g, lo = try_flat(start, stride, idf, pf)
        frac = g / r if r else 0
        if best is None or frac > best[0]:
            best = (frac, name, start, r, g, lo)
        if frac > 0.5:
            print(f"  {name:22} start={start}: {g}/{r} ({frac:.0%}) leftover {lo}")
print(f"\nbest overall: {best[1]} start={best[2]} -> {best[4]}/{best[3]} ({best[0]:.0%}) leftover {best[5]}")

# Grouped: [src u32][sp u8][cnt u8] then cnt*[dst u32][dp u8], various heads.
def try_grouped(start):
    off, links = start, []
    while off + 6 <= n:
        src, sp, cnt = U(off), raw[off + 4], raw[off + 5]
        if not vid(src) or not (1 <= cnt <= 64) or off + 6 + cnt * 5 > n:
            break
        off += 6
        for _ in range(cnt):
            links.append((src, sp, U(off), raw[off + 4]))
            off += 5
    return links, n - off


print("\n== grouped [src,sp,cnt, cnt*(dst,dp)] ==")
for start in range(0, 6):
    links, lo = try_grouped(start)
    v = sum(1 for s, _, d, _ in links if vid(s) and vid(d))
    if links:
        print(f"  start={start}: {len(links)} links, {v} valid, leftover {lo}")

# Assumption-free: where do valid device ids actually appear, and at what spacing?
hits = [o for o in range(0, n - 3) if vid(U(o))]
gaps = Counter(hits[i + 1] - hits[i] for i in range(len(hits) - 1))
print(f"\n== id-occurrence analysis: {len(hits)} offsets hold a valid id ==")
print("  top gaps between consecutive id-offsets:", gaps.most_common(8))
print("  first 40 ids in blob order:",
      [byid[U(o)].cc for o in hits[:40]])
# Confirmed stride 9. Dump records for header 0 and 1; score id positions.
for hdr in (0, 1):
    off, recs = hdr, []
    while off + 9 <= n:
        recs.append((U(off), U(off + 4), raw[off + 8]))
        off += 9
    a_ok = sum(1 for a, b, f in recs if a in idset)
    b_ok = sum(1 for a, b, f in recs if b in idset)
    print(f"\nhdr={hdr}: {len(recs)} recs, a-field valid {a_ok}, b-field valid {b_ok}, leftover {n-off}")

print("\n== 9-byte record dump (hdr=1), first 26 ==")
off = 1
for _ in range(26):
    a, b, fl = U(off), U(off + 4), raw[off + 8]
    ca = byid[a].cc if a in idset else "."
    cb = byid[b].cc if b in idset else "."
    print(f"  @{off:4d} {raw[off:off+9].hex()}  a={a:<6}({ca:5}) b={b:<10}({cb:5}) fl={fl}")
    off += 9
