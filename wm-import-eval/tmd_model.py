#!/usr/bin/env python3
"""Structured model of a World Machine .tmd device world.

Turns the raw block tree (tmd_parse) into a list of Device records carrying the
fields we care about for translation: type FourCC, label, canvas position,
internal id, the raw PAR2 param blob, and the printable strings embedded in it
(which expose enum option labels + param names = WM's config surface).
"""
import struct
import re
import tmd_parse as T

PRINTABLE = re.compile(rb"[\x20-\x7e]{4,}")


class Device:
    __slots__ = ("idx", "cc", "name", "pos", "iid", "params", "strings", "contents")

    def __init__(self, idx, cc, name, pos, iid, params):
        self.idx = idx
        self.cc = cc
        self.name = name
        self.pos = pos
        self.iid = iid
        self.params = params
        self.strings = []
        self.contents = b""  # full raw Contents bytes (PAR2 lives here even for
        #                      inline-format devices whose Contents stays a leaf)

    def par2(self):
        """Best blob to decode params from: the parsed `params` leaf when the
        Contents was a container, else the whole raw Contents (inline format)."""
        return self.params if self.params else self.contents

    def __repr__(self):
        return f"<#{self.idx} {self.cc} {self.name!r} id={self.iid} pos={self.pos}>"


def _clean_name(basic_raw):
    """basic = posx:i32, posy:i32, NUL-terminated name, trailing flags."""
    if basic_raw is None or len(basic_raw) < 8:
        return None, None
    px, py = struct.unpack_from("<ii", basic_raw, 0)
    tail = basic_raw[8:]
    nul = tail.find(b"\x00")
    name = tail[:nul if nul >= 0 else len(tail)].decode("latin-1", "replace")
    name = name.strip()
    return (name or None), (px, py)


def load(path):
    roots, buf, end = T.parse_file(path, max_roots=1)
    world = roots[0]
    devices_blk = world.child("Devices")
    out = []
    for i, d in enumerate(devices_blk.children):
        idblk = d.child("ID")
        cc = idblk.raw.decode("latin-1") if idblk and idblk.raw else "????"
        contents = d.child("Contents")
        contents_raw = b""
        if contents is not None:
            contents_raw = contents.raw if contents.raw is not None else b""
        device = contents.children[0] if contents and contents.children else None
        name = pos = iid = None
        params = b""
        if device is not None:
            name, pos = _clean_name(_leaf(device, "basic"))
            idraw = _leaf(device, "id")
            if idraw and len(idraw) >= 4:
                iid = struct.unpack_from("<i", idraw, 0)[0]
            params = _leaf(device, "params") or b""
        dev = Device(i, cc, name, pos, iid, params)
        dev.contents = contents_raw
        # Inline-format devices (e.g. APRL) leave Contents a leaf: pull the name
        # that follows the embedded "basic" tag.
        if name is None and contents_raw:
            b = contents_raw.find(b"basic")
            if b >= 0:
                m = PRINTABLE.search(contents_raw, b + 5)
                if m and m.start() < b + 64:
                    dev.name = m.group().decode("latin-1").strip()
        # Embedded strings from the best param blob (catches inline format too).
        dev.strings = [m.decode("latin-1") for m in PRINTABLE.findall(dev.par2())]
        out.append(dev)
    return world, out


def _leaf(device, field):
    b = device.child(field)
    if b is None:
        return None
    return b.raw


if __name__ == "__main__":
    import sys
    from collections import Counter, defaultdict
    path = sys.argv[1]
    world, devs = load(path)
    fourcc = Counter(d.cc for d in devs)
    names = defaultdict(list)
    strings = defaultdict(Counter)
    for d in devs:
        if d.name:
            names[d.cc].append(d.name)
        for s in d.strings:
            if s not in ("PAR2",):
                strings[d.cc][s] += 1
    print(f"# {path}")
    print(f"# {len(devs)} devices, {len(fourcc)} distinct types\n")
    for cc, n in fourcc.most_common():
        ex = "; ".join(dict.fromkeys(names.get(cc, [])))[:70]
        print(f"{n:>4}  {cc:5}  {ex}")
        common_strs = [s for s, _ in strings[cc].most_common(8)
                       if not s.startswith("P1") and len(s) >= 4]
        if common_strs:
            print(f"          params: {' | '.join(common_strs[:8])[:110]}")
