#!/usr/bin/env python3
"""Ad-hoc exploration: dump the first N Device2 blocks in full depth."""
import sys
import tmd_parse as T


def find(blk, name):
    if blk.name == name:
        return blk
    for c in blk.children:
        r = find(c, name)
        if r:
            return r
    return None


def full(blk, depth=0, maxdepth=12):
    ind = "  " * depth
    if blk.children:
        pfx = f" pfx={blk.prefix.hex()}" if blk.prefix else ""
        print(f"{ind}{blk.name} [{blk.size}B{pfx}]")
        if depth < maxdepth:
            for c in blk.children:
                full(c, depth + 1, maxdepth)
    else:
        print(f"{ind}{blk.name}  {T.preview_leaf(blk.raw or b'')}")


if __name__ == "__main__":
    path = sys.argv[1]
    ndev = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    roots, buf, end = T.parse_file(path)
    devices = find(roots[0], "Devices")
    print(f"# Devices: {len(devices.children)} Device2 blocks")
    for i, d in enumerate(devices.children[:ndev]):
        print(f"\n===== Device #{i} =====")
        full(d)
