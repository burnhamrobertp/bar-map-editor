#!/usr/bin/env python3
"""Annotated dump of a device's PAR2 param blob, to decode its structure.

Shows, per 4-byte word: offset, raw hex, u32, i32, f32, and flags any ASCII
string regions. Run against several instances of one device type and diff the
varying words (= values) against the constant ones (= schema/labels)."""
import sys
import struct
import tmd_model as M


def find_strings(raw, minlen=3):
    spans = []
    i = 0
    n = len(raw)
    while i < n:
        if 32 <= raw[i] < 127:
            j = i
            while j < n and 32 <= raw[j] < 127:
                j += 1
            if j - i >= minlen:
                spans.append((i, j, raw[i:j].decode("latin-1")))
            i = j
        else:
            i += 1
    return spans


def dump(raw, limit=None):
    n = len(raw)
    spans = find_strings(raw)
    strstarts = {s[0]: s for s in spans}
    instr = set()
    for a, b, _ in spans:
        instr.update(range(a, b))
    print(f"  PAR2 blob {n} bytes")
    print(f"  strings: " + " | ".join(f"@{a}:{t!r}" for a, b, t in spans))
    end = n if limit is None else min(n, limit)
    off = 0
    while off + 4 <= end:
        word = raw[off:off + 4]
        u = struct.unpack("<I", word)[0]
        i = struct.unpack("<i", word)[0]
        f = struct.unpack("<f", word)[0]
        tag = ""
        if off in strstarts:
            a, b, t = strstarts[off]
            tag = f"  <STR {t!r}>"
        elif off in instr:
            tag = "  <str..>"
        fdisp = f"{f:.5g}" if (abs(f) < 1e9 and (f == 0 or abs(f) > 1e-6)) else "."
        print(f"   @{off:4d} {word.hex()}  u={u:<11} i={i:<11} f={fdisp}{tag}")
        off += 4


def main():
    path = sys.argv[1]
    cc = sys.argv[2]
    which = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    limit = int(sys.argv[4]) if len(sys.argv) > 4 else 80
    w, devs = M.load(path)
    insts = [d for d in devs if d.cc == cc]
    print(f"# {cc}: {len(insts)} instances; showing #{which} (name={insts[which].name!r})")
    dump(insts[which].params, limit)


if __name__ == "__main__":
    main()
