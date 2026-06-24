#!/usr/bin/env python3
"""Parser for World Machine .tmd (TMDFile2) save files.

The format is a tree of named "WM" blocks:

    magic   = b"TMDFile2"
    block   = b"WM" + name_len:u8 + name + size:u64le + payload[size]
    payload = either a sequence of child blocks (container) or raw bytes (leaf)

This first pass just walks the tree and prints structure + leaf previews so we
can learn the layout empirically. Run:  python tmd_parse.py <file.tmd> [maxdepth]
"""
import sys
import struct

MAGIC = b"TMDFile2"


class Block:
    __slots__ = ("name", "start", "size", "payload_start", "children", "raw", "prefix")

    def __init__(self, name, start, size, payload_start):
        self.name = name
        self.start = start
        self.size = size
        self.payload_start = payload_start
        self.children = []
        self.raw = None    # bytes if leaf
        self.prefix = b""  # bytes consumed before children (count / FourCC+ver)

    def child(self, name):
        for c in self.children:
            if c.name == name:
                return c
        return None


def looks_like_block(buf, pos):
    return pos + 3 <= len(buf) and buf[pos:pos + 2] == b"WM"


# Top-level devices and their leaf fields all sit at depth <=4 (Devices >
# Device2 > Contents > Device > basic/params). Capping at 6 reaches them while
# refusing to descend into nested-macro sub-worlds -- which multiply into
# thousands of blocks and are not part of the top-level device inventory we map.
MAX_DEPTH = 6

# Device leaf fields are always raw PAR2/blob data, never child-block containers.
# Probing them wastes time matching coincidental "WM" bytes in curve LUTs and
# baked data (and can spuriously tile). Treat them as leaves unconditionally.
ALWAYS_LEAF = {"params", "basic", "context", "origin", "id", "defaultout"}


def parse_block(buf, pos, depth=0):
    """Parse one WM block starting at pos. Returns (Block, next_pos)."""
    assert buf[pos:pos + 2] == b"WM", f"no WM marker at {pos:#x}"
    p = pos + 2
    name_len = buf[p]
    p += 1
    name = buf[p:p + name_len].decode("latin-1")
    p += name_len
    size = struct.unpack_from("<Q", buf, p)[0]
    p += 8
    payload_start = p
    blk = Block(name, pos, size, payload_start)
    payload_end = payload_start + size

    # Container forms have a (possibly empty) fixed prefix before the child
    # blocks, then children that tile the remaining payload exactly. Known
    # prefixes: none; a u32 count; a 4-byte FourCC + 1 version byte (device
    # Contents). Probe small prefix lengths and take the first that tiles.
    # Prefix lengths that actually occur: none, u32 count, FourCC+ver (5),
    # FourCC+ver+u32count (9). Probing more inflates false-positive container
    # detection (and parse time) on big baked-data blocks.
    probes = () if (depth >= MAX_DEPTH or name in ALWAYS_LEAF) else (
        payload_start, payload_start + 4, payload_start + 5, payload_start + 9)
    for child_start in probes:
        if child_start > payload_end:
            continue
        if not looks_like_block(buf, child_start):
            continue
        cp = child_start
        ok = True
        kids = []
        while cp < payload_end:
            if not looks_like_block(buf, cp) or len(kids) > 5_000:
                ok = False
                break
            try:
                child, cp = parse_block(buf, cp, depth + 1)
            except (AssertionError, struct.error, IndexError):
                ok = False
                break
            kids.append(child)
        if ok and cp == payload_end and kids:
            blk.children = kids
            blk.prefix = buf[payload_start:child_start]
            return blk, payload_end
    # Leaf
    blk.raw = buf[payload_start:payload_end]
    return blk, payload_end


def parse_file(path, max_roots=None):
    """Parse the WM block tree. `max_roots` stops after that many top-level
    blocks -- the device graph lives entirely in the first root (DEVICEWORLD),
    so callers that only need devices pass max_roots=1 to skip the multi-MB
    baked-terrain sibling blocks that follow it in larger saves."""
    with open(path, "rb") as f:
        buf = f.read()
    assert buf[:8] == MAGIC, f"bad magic: {buf[:8]!r}"
    pos = 8
    roots = []
    while pos < len(buf) and looks_like_block(buf, pos):
        blk, pos = parse_block(buf, pos)
        roots.append(blk)
        if max_roots is not None and len(roots) >= max_roots:
            break
    return roots, buf, pos


def preview_leaf(raw):
    n = len(raw)
    out = [f"{n}B"]
    # ASCII string?
    if n and all(32 <= b < 127 for b in raw[:min(n, 64)]) and raw[:1] != b"\x00":
        try:
            out.append(f'str="{raw[:64].decode("latin-1")}"')
            return " ".join(out)
        except Exception:
            pass
    if n == 4:
        out.append(f"f32={struct.unpack('<f', raw)[0]:.5g} i32={struct.unpack('<i', raw)[0]}")
    elif n == 8:
        out.append(f"f64={struct.unpack('<d', raw)[0]:.5g} i64={struct.unpack('<q', raw)[0]}")
    elif n == 12:
        out.append("vec3=" + ",".join(f"{x:.4g}" for x in struct.unpack('<3f', raw)))
    else:
        out.append("hex=" + raw[:24].hex())
    return " ".join(out)


MAX_CHILDREN = 9999


def dump(blk, depth, maxdepth, counts):
    counts[blk.name] = counts.get(blk.name, 0) + 1
    indent = "  " * depth
    if blk.children:
        pfx = f" pfx={blk.prefix.hex()}" if blk.prefix else ""
        print(f"{indent}{blk.name} [{blk.size}B, {len(blk.children)} children{pfx}]")
        if depth < maxdepth:
            for c in blk.children[:MAX_CHILDREN]:
                dump(c, depth + 1, maxdepth, counts)
            if len(blk.children) > MAX_CHILDREN:
                print(f"{indent}  ... +{len(blk.children) - MAX_CHILDREN} more")
    else:
        print(f"{indent}{blk.name}  {preview_leaf(blk.raw or b'')}")


if __name__ == "__main__":
    path = sys.argv[1]
    maxdepth = int(sys.argv[2]) if len(sys.argv) > 2 else 4
    if len(sys.argv) > 3:
        MAX_CHILDREN = int(sys.argv[3])
    roots, buf, end = parse_file(path)
    print(f"# {path}: {len(buf)} bytes, {len(roots)} root block(s), parsed to {end:#x}")
    counts = {}
    for r in roots:
        dump(r, 0, maxdepth, counts)
    print("\n# block name counts:")
    for name, c in sorted(counts.items(), key=lambda kv: -kv[1]):
        print(f"  {c:6d}  {name}")
