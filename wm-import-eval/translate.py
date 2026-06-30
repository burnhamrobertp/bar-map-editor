#!/usr/bin/env python3
"""Translate a World Machine .tmd into (1) a per-device coverage report and
(2) a runnable bar-editor .barproj recreation.

The coverage report is the honest, complete artifact: every WM device mapped to
its bar node + gap status, straight from the real parse. The .barproj is a
representative, schema-valid recreation of the map's terrain pipeline using the
bar node types that correspond to the map's dominant devices, with identity /
water / sun settings lifted from the .tmd itself.
"""
import sys
import json
import struct
import os
from collections import Counter

import tmd_parse as T
import tmd_model as M
import mapping
import bar_schema


def world_meta(world):
    meta = {}
    for name, conv in (("Author", "str"), ("Description", "str"),
                       ("DefaultPath", "str"), ("Waterlevel", "wl"),
                       ("Sunlight", "vec3")):
        blk = world.child(name)
        if not blk or blk.raw is None:
            continue
        raw = blk.raw
        if conv == "str":
            meta[name] = raw.split(b"\x00", 1)[0].decode("latin-1", "replace").strip()
        elif conv == "wl" and len(raw) >= 5:
            meta[name] = round(struct.unpack_from("<f", raw, 1)[0], 4)
        elif conv == "vec3" and len(raw) >= 12:
            meta[name] = [round(x, 4) for x in struct.unpack_from("<3f", raw, 0)]
    return meta


def coverage(devs):
    by_type = Counter(d.cc for d in devs)
    rows = []
    status_tot = Counter()
    for cc, n in by_type.most_common():
        wm_name, node, status, note = mapping.lookup(cc)
        status_tot[status] += n
        rows.append({"cc": cc, "count": n, "wm_device": wm_name,
                     "bar_node": node, "status": status, "note": note})
    return rows, status_tot, by_type


def node(key, ntype, label, **params):
    p = {}
    for k, v in params.items():
        p[k] = v
    return {"key": key, "type": ntype, "label": label, "params": p}


def F(x): return {"Float": x}
def U(x): return {"UInt": x}
def S(x): return {"String": x}


def build_recipe(mapname, meta, by_type):
    """A representative BAR terrain pipeline built from bar nodes that mirror
    the device classes actually present in the source graph."""
    has_erosion = (by_type.get("ERD2", 0) + by_type.get("EWTH", 0)) > 0
    wl = meta.get("Waterlevel", 0.05)
    sun = meta.get("Sunlight", [0.47, 0.6, 0.64])

    nodes = [
        node("base", "PerlinNoise", "Base terrain (APRL)",
             character=S("rugged"), frequency=F(3.0), octaves=U(7), seed=U(1337)),
        node("ridges", "RidgedNoise", "Ridge network (APRL/CMB2)",
             character=S("ridges"), frequency=F(2.0), octaves=U(6), seed=U(7)),
        node("combine", "Blend", "Combiner (CMB2: blend)", factor=F(0.4)),
    ]
    conns = [
        {"from": "base.output", "to": "combine.a"},
        {"from": "ridges.output", "to": "combine.b"},
    ]
    height_src = "combine"

    if has_erosion:
        nodes.append(node("erode", "HydraulicErosion", "Hydraulic erosion (ERD2)",
                          iterations=U(120000), erosion_rate=F(0.03), deposition_rate=F(0.02)))
        conns.append({"from": f"{height_src}.output", "to": "erode.input"})
        height_src = "erode"

    nodes.append(node("clamp", "Clamp", "Clamp to Spring height (CLMP)",
                      min=F(0.0), max=F(1.0)))
    conns.append({"from": f"{height_src}.output", "to": "clamp.input"})
    height_src = "clamp"

    # Derived analysis + texture/maps
    nodes += [
        node("slope", "SlopeMap", "Slope map (SSEL source)"),
        node("tex", "AutoTexture", "Auto texture (PRCC/OLVW)",
             biome=S("temperate"), slope_blend=F(1.0)),
        node("nrm", "NormalMap", "Normal map (PRNO)", strength=F(1.0)),
        node("spec", "SpecularMap", "Specular (PRLM-ish)"),
        node("grass", "GrassMap", "Grass density (LAYG/HSEL mask)"),
        node("metal", "Layout", "Metal spots (LAYG)", item_count=U(2), mode=S("mask")),
        node("type", "HeightSelect", "Typemap band (HSPL/HSEL)",
             low=F(0.0), high=F(0.45), falloff=F(0.1)),
        node("fc", "FinalComposition", "Output (OUTP/PRBO)"),
    ]
    conns += [
        {"from": f"{height_src}.output", "to": "slope.input"},
        {"from": f"{height_src}.output", "to": "tex.input"},
        {"from": "slope.output", "to": "tex.slope"},
        {"from": f"{height_src}.output", "to": "nrm.input"},
        {"from": f"{height_src}.output", "to": "spec.input"},
        {"from": "slope.output", "to": "spec.slope"},
        {"from": f"{height_src}.output", "to": "grass.input"},
        {"from": "slope.output", "to": "grass.slope"},
        {"from": f"{height_src}.output", "to": "type.input"},
        # FinalComposition channels
        {"from": f"{height_src}.output", "to": "fc.heightmap"},
        {"from": "tex.output", "to": "fc.texture"},
        {"from": "nrm.output", "to": "fc.normalmap"},
        {"from": "spec.output", "to": "fc.specular"},
        {"from": "grass.output", "to": "fc.grassmap"},
        {"from": "metal.output", "to": "fc.metalmap"},
        {"from": "type.output", "to": "fc.typemap"},
    ]

    desc = (f"bar-editor recreation of World Machine map '{mapname}'. "
            f"Representative terrain pipeline (the WM source has hundreds of devices; "
            f"see coverage report). WM author: {meta.get('Author', '?')}. "
            f"WM water level {wl}, sun dir {sun}.")

    recipe = {
        "name": mapname,
        "author": meta.get("Author") or "bar-editor",
        "description": desc,
        "nodes": nodes,
        "connections": conns,
        "output": {
            "width": 513,
            "height": 513,
            "map_settings": {
                "min_height": round(-200.0 * wl - 30, 1),
                "max_height": 480.0,
                "lighting": {"sun_dir": sun},
                "water": {"base_color": [0.4, 0.55, 0.75]},
                "start_positions": [[64, 256], [448, 256]],
            },
        },
    }
    return recipe


def main():
    path = sys.argv[1]
    outdir = sys.argv[2] if len(sys.argv) > 2 else os.path.dirname(os.path.abspath(__file__))
    stem = os.path.basename(path)
    mapname = stem.rsplit(".", 1)[0].split("-", 1)[-1]

    world, devs = M.load(path)
    meta = world_meta(world)
    rows, status_tot, by_type = coverage(devs)
    total = sum(status_tot.values())

    cov = {
        "source": stem,
        "map_name": mapname,
        "wm_meta": meta,
        "device_total": total,
        "distinct_types": len(by_type),
        "status_totals": dict(status_tot),
        "status_pct": {k: round(100 * v / total, 1) for k, v in status_tot.items()},
        "types": rows,
    }
    cov_path = os.path.join(outdir, f"{mapname}.coverage.json")
    with open(cov_path, "w") as f:
        json.dump(cov, f, indent=2)

    recipe = build_recipe(mapname, meta, by_type)
    errs = bar_schema.validate_recipe(recipe)
    rec_path = os.path.join(outdir, f"{mapname}.barproj")
    with open(rec_path, "w") as f:
        json.dump(recipe, f, indent=2)

    print(f"# {mapname}: {total} devices, {len(by_type)} types")
    print(f"  meta: {meta}")
    print(f"  coverage (by device instance): "
          + ", ".join(f"{k} {v} ({cov['status_pct'][k]}%)" for k, v in status_tot.most_common()))
    print(f"  wrote {os.path.basename(cov_path)}, {os.path.basename(rec_path)}")
    print(f"  recipe schema validation: {'OK' if not errs else 'FAIL'}")
    for e in errs:
        print("     -", e)


if __name__ == "__main__":
    main()
