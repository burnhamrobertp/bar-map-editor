#!/usr/bin/env python3
"""Dump raw bytes of Device sub-fields for specific device indices."""
import sys
import struct
import tmd_parse as T


def show(path, indices):
    roots, buf, end = T.parse_file(path)
    devices = roots[0].child("Devices")
    for i in indices:
        d = devices.children[i]
        cc = d.child("ID").raw.decode("latin-1")
        dev = d.child("Contents").children[0]
        print(f"\n===== #{i}  ID={cc}  (Contents pfx={d.child('Contents').prefix.hex()}) =====")
        for f in ("basic", "origin", "id", "params", "context", "defaultout"):
            b = dev.child(f)
            if b is None:
                continue
            raw = b.raw if b.raw is not None else b"<container>"
            if isinstance(raw, bytes):
                print(f"  {f} ({len(raw)}B): {raw[:96].hex()}")
                printable = "".join(chr(x) if 32 <= x < 127 else "." for x in raw[:96])
                print(f"        ascii: {printable}")


if __name__ == "__main__":
    path = sys.argv[1]
    idx = [int(x) for x in sys.argv[2:]] or [0, 1]
    show(path, idx)
