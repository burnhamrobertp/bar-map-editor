#!/usr/bin/env python3
"""Decode a WM PAR2 param blob into [(name, value)] pairs.

Layout learned empirically (see REPORT / par2_dump.py):
  header = b"\\xce\\xff" + b"PAR2" + version:u8 + count:u32
  then `count` fixed parameter records, each beginning with the 3-byte marker
  b"P1\\x07" immediately followed by the 4-byte value. The parameter's name is a
  fixed-width field later in the record; in the blob it shows up as the first
  printable string after the value. The marker recurs exactly once per param
  (verified: marker count == declared count for every device type seen).

Values are 4 bytes; interpreted as f32 unless that yields a denormal/huge number
and the int reading is small (enums / bools / counts).
"""
import struct
import re

MARKER = b"\x50\x31\x07"  # "P1" + type tag 0x07
NAME_AT = 59              # the param's name field sits at marker + 59, universally
NAME_RE = re.compile(rb"[A-Z][\x20-\x7e]{2,}")  # real param names are Title-case


def _interpret(b4):
    u = struct.unpack("<I", b4)[0]
    i = struct.unpack("<i", b4)[0]
    f = struct.unpack("<f", b4)[0]
    # Prefer float when it is a normal, sane magnitude; else fall back to int.
    if f == 0.0:
        return 0.0 if u == 0 else f
    if 1e-4 <= abs(f) <= 1e6:
        return round(f, 5)
    return i


def decode(blob):
    """Return ([(name, value)], declared_count). Each real parameter record
    begins with MARKER, carries its value at +3 and its name at +59. The marker
    byte sequence occasionally occurs by chance inside value/metadata data;
    those false hits have no valid name field at +59 and are dropped."""
    h = blob.find(b"PAR2")
    if h < 0:
        return [], 0
    # header = ...PAR2 + version:u8 + count:u32; count sits 5 bytes past "PAR2".
    count = struct.unpack_from("<I", blob, h + 5)[0] if h + 9 <= len(blob) else 0
    if count > 1000:
        count = 0  # embedded/inline blob -- header offset unreliable; ignore
    out = []
    i = 0
    while True:
        m = blob.find(MARKER, i)
        if m < 0:
            break
        i = m + 3
        nm = NAME_RE.match(blob, m + NAME_AT, m + NAME_AT + 36)
        if not nm:
            continue  # false marker
        val = _interpret(blob[m + 3:m + 7])
        out.append((nm.group().decode("latin-1"), val))
    return out, count


if __name__ == "__main__":
    import sys
    import tmd_model as M
    path, cc = sys.argv[1], sys.argv[2]
    which = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    w, devs = M.load(path)
    insts = [d for d in devs if d.cc == cc]
    d = insts[which]
    params, count = decode(d.par2())
    print(f"# {cc} #{which} (name={d.name!r})  declared count={count}, decoded {len(params)}")
    for name, val in params:
        print(f"   {name:24} = {val}")
