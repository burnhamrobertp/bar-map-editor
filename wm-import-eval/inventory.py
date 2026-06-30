#!/usr/bin/env python3
"""Device-type inventory across .tmd files: FourCC histogram + names."""
import sys
import struct
from collections import Counter, defaultdict
import tmd_parse as T


def device_name(dev):
    """Pull the human label out of Device/basic (posx:i32, posy:i32, name)."""
    contents = dev.child("Contents")
    if not contents or not contents.children:
        return None, None
    device = contents.children[0]
    basic = device.child("basic")
    if not basic or basic.raw is None or len(basic.raw) < 8:
        return None, None
    px, py = struct.unpack_from("<ii", basic.raw, 0)
    # Name: printable run after the two ints.
    tail = basic.raw[8:]
    name = "".join(chr(b) for b in tail if 32 <= b < 127).strip("\x00 ")
    return name or None, (px, py)


def device_id(dev):
    contents = dev.child("Contents")
    device = contents.children[0] if contents and contents.children else None
    if device:
        idblk = device.child("id")
        if idblk and idblk.raw and len(idblk.raw) >= 4:
            return struct.unpack_from("<i", idblk.raw, 0)[0]
    return None


def inventory(path):
    roots, buf, end = T.parse_file(path)
    world = roots[0]
    devices = world.child("Devices")
    fourcc = Counter()
    names = defaultdict(list)
    rows = []
    for d in devices.children:
        idblk = d.child("ID")
        cc = idblk.raw.decode("latin-1") if idblk and idblk.raw else "????"
        fourcc[cc] += 1
        nm, pos = device_name(d)
        did = device_id(d)
        rows.append((did, cc, nm, pos))
        if nm:
            names[cc].append(nm)
    return world, devices, fourcc, names, rows


if __name__ == "__main__":
    path = sys.argv[1]
    world, devices, fourcc, names, rows = inventory(path)
    print(f"# {path}")
    print(f"# {len(devices.children)} devices, {len(fourcc)} distinct types\n")
    print(f"{'count':>5}  {'cc':5}  example names")
    for cc, n in fourcc.most_common():
        ex = names.get(cc, [])
        sample = "; ".join(dict.fromkeys(ex))[:80]
        print(f"{n:>5}  {cc:5}  {sample}")
