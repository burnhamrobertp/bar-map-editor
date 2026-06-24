#!/usr/bin/env python3
"""Decode the newer WM 'ParmGroup' param format (Aethermoor / newer WM builds).

Unlike the flat PAR2 blob (ATG2), this format is a self-describing tree of WM
blocks:  params > ParmGroup > { ver, numParams, <Parm blocks> }, where each Parm
carries `name`, `type` ("float"/"int"/...), and a value sub-block. We walk the
WM tree directly and pull (name, value) per Parm."""
import struct
import tmd_model as M
import tmd_parse as T


def walk(buf, pos, end, depth, out):
    while pos < end - 3:
        if buf[pos:pos + 2] != b"WM":
            pos += 1
            continue
        nl = buf[pos + 2]
        name = buf[pos + 3:pos + 3 + nl].decode("latin-1", "replace")
        p = pos + 3 + nl
        if p + 8 > end:
            break
        sz = struct.unpack_from("<Q", buf, p)[0]
        p += 8
        payload = buf[p:p + sz]
        is_container = payload[:2] == b"WM"
        out.append((depth, name, sz, None if is_container else payload))
        if is_container and depth < 8:
            walk(buf, p, p + sz, depth + 1, out)
        pos = p + sz
    return pos


def dump_tree(blob):
    out = []
    walk(blob, 0, len(blob), 0, out)
    for depth, name, sz, payload in out:
        prev = ""
        if payload is not None:
            if sz == 4:
                prev = "f32=%.5g i32=%d" % (struct.unpack("<f", payload)[0],
                                            struct.unpack("<i", payload)[0])
            elif sz <= 24:
                prev = payload.hex()
                txt = "".join(chr(b) if 32 <= b < 127 else "." for b in payload)
                prev += f"  '{txt}'"
            else:
                prev = "%dB" % sz
        print("  " * depth + f"{name} [{sz}B] {prev}")


def decode(blob):
    """Return [(param_name, value)] from a ParmGroup blob.

    Each P4FULL record exposes a `name` leaf then a value leaf nested in `val`:
    `float` (f32), `int` (i32), or `enum`>`choice` (selected index)."""
    out = []
    walk(blob, 0, len(blob), 0, out)
    params = []
    cur_name = None
    awaiting = False  # have a name, still looking for its value leaf
    for depth, name, sz, payload in out:
        if name == "name" and payload is not None and sz < 64:
            cur_name = payload.split(b"\x00", 1)[0].decode("latin-1", "replace")
            awaiting = True
        elif awaiting and payload is not None and sz == 4 and name in ("float", "int", "choice"):
            if name == "float":
                v = round(struct.unpack("<f", payload)[0], 5)
            else:
                v = struct.unpack("<i", payload)[0]
            params.append((cur_name, v))
            awaiting = False
    return params


if __name__ == "__main__":
    import sys
    path, cc = sys.argv[1], sys.argv[2]
    which = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    mode = sys.argv[4] if len(sys.argv) > 4 else "tree"
    w, devs = M.load(path)
    insts = [d for d in devs if d.cc == cc]
    d = insts[which]
    print(f"# {cc} #{which} name={d.name!r} params={len(d.params)}B")
    if mode == "tree":
        dump_tree(d.params)
    else:
        for nm, v in decode(d.params):
            print(f"   {nm:24} = {v}")
