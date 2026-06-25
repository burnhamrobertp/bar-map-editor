#!/usr/bin/env python3
"""Cross-file corpus summary over every .tmd in ~/Downloads.

Per map: device count, distinct types, device-instance coverage (good/partial/
gap), and PAR2/ParmGroup decode success. Plus the corpus-wide device-type union
and any FourCC not yet in mapping.py."""
import glob
import os
from collections import Counter
import tmd_model as M
import mapping
from compare import decode_any

DOWNLOADS = os.path.expanduser("~/Downloads")


def files():
    return sorted(glob.glob(os.path.join(DOWNLOADS, "*.tmd")))


def map_name(path):
    return os.path.basename(path).rsplit(".", 1)[0].split("-", 1)[-1]


def analyze(path):
    w, devs = M.load(path)
    types = Counter(d.cc for d in devs)
    status = Counter()
    dec_ok = dec_tot = 0
    for d in devs:
        status[mapping.lookup(d.cc)[2]] += 1
        if d.cc in ("OUTP", "CHKP", "MACR"):  # paramless / structural; skip decode metric
            continue
        dec_tot += 1
        if decode_any(d):
            dec_ok += 1
    return {
        "name": map_name(path), "devices": len(devs), "types": len(types),
        "status": status, "type_counter": types,
        "decode_pct": round(100 * dec_ok / dec_tot, 0) if dec_tot else 0,
    }


def main():
    fs = files()
    rows = [analyze(f) for f in fs]
    union = Counter()
    for r in rows:
        union.update(r["type_counter"])

    print(f"# Corpus: {len(fs)} World Machine maps\n")
    print(f"{'map':18} {'devs':>5} {'types':>5} {'good%':>6} {'part%':>6} {'gap%':>5} {'decode%':>7}")
    tot = Counter()
    for r in rows:
        s = r["status"]
        n = sum(s.values())
        tot.update(s)
        g, p, x = (round(100 * s[k] / n) for k in ("good", "partial", "gap"))
        print(f"{r['name']:18} {r['devices']:>5} {r['types']:>5} {g:>5}% {p:>5}% {x:>4}% {r['decode_pct']:>6.0f}%")
    N = sum(tot.values())
    print(f"{'TOTAL':18} {N:>5} {len(union):>5} "
          f"{round(100*tot['good']/N):>5}% {round(100*tot['partial']/N):>5}% {round(100*tot['gap']/N):>4}%")

    unmapped = [cc for cc in union if cc not in mapping.MAP]
    print(f"\n# distinct device types across corpus: {len(union)}")
    print(f"# device instances total: {N}")
    if unmapped:
        print(f"\n# UNMAPPED device types (not in mapping.py): {unmapped}")
        for cc in unmapped:
            print(f"   {cc}: {union[cc]} uses")
    else:
        print("\n# all device types are covered by mapping.py")

    print("\n# top device types corpus-wide:")
    for cc, n in union.most_common(20):
        nm, node, st, _ = mapping.lookup(cc)
        print(f"  {n:5}  {cc:5} {st:8} {nm}")


if __name__ == "__main__":
    main()
