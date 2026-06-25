#!/usr/bin/env python3
"""Param-faithful recreation of a *decodable* WM .tmd (ATG2/onyx2/BSR).

Unlike translate.py's fixed template, this decodes the map's real APRL
generators (Scale/Persistence/Lacunarity/Seed/Steepness/Elevation) and ERD2
erosion, and builds a layered terrain from those actual values. Topology is
NOT recovered (the WM Links block is port-handle indirection); the assembly is
a principled base+detail+erosion stack populated with the map's real numbers,
so feature scale / roughness / erosion character reflect the source.

Usage: python translate_faithful.py <map.tmd> <out.json>
"""
import sys
import json
import struct
import tmd_model as M
import par2_decode as P


def world_meta(world):
    meta = {}
    wl = world.child("Waterlevel")
    if wl and wl.raw and len(wl.raw) >= 5:
        meta["water"] = round(struct.unpack_from("<f", wl.raw, 1)[0], 4)
    sun = world.child("Sunlight")
    if sun and sun.raw and len(sun.raw) >= 12:
        meta["sun"] = [round(x, 4) for x in struct.unpack_from("<3f", sun.raw, 0)]
    au = world.child("Author")
    meta["author"] = au.raw.split(b"\x00", 1)[0].decode("latin-1", "replace").strip() if au and au.raw else "?"
    return meta


def decode_dev(d):
    return dict(P.decode(d.par2())[0])


def clamp(v, lo, hi):
    return max(lo, min(hi, v))


def F(x): return {"Float": round(float(x), 5)}
def U(x): return {"UInt": int(x)}
def S(x): return {"String": x}


def gen_node(key, label, p):
    """APRL decoded dict -> bar PerlinNoise params (real values)."""
    scale = p.get("Scale", 0.04) or 0.04
    freq = clamp(1.0 / scale if scale > 0 else 4.0, 0.1, 128.0)
    return {
        "key": key, "type": "PerlinNoise", "label": label,
        "params": {
            "frequency": F(freq),
            "octaves": U(clamp(int(p.get("Octaves") or 8) or 8, 1, 12)),
            "lacunarity": F(clamp(p.get("Lacunarity", 2.0), 1.0, 4.0)),
            "persistence": F(clamp(p.get("Persistence", 0.5), 0.0, 1.0)),
            "seed": U(int(p.get("Seed", 0)) & 0xFFFFFFFF),
            "steepness": F(clamp(p.get("Steepness", 0.5), 0.0, 1.0)),
            "elevation": F(clamp(p.get("Elevation", 0.5), 0.0, 1.0)),
        },
        "_freq": freq,
    }


def build(mapname, world, devs):
    meta = world_meta(world)
    aprl = [d for d in devs if d.cc == "APRL"]
    gens = []
    for i, d in enumerate(aprl):
        p = decode_dev(d)
        if "Scale" not in p:
            continue
        gens.append(gen_node(f"g{i}", (d.name or f"APRL {i}")[:40], p))
    # Sort by feature size: lowest frequency (biggest landform) first.
    gens.sort(key=lambda g: g["_freq"])
    if len(gens) < 2:
        raise SystemExit(f"{mapname}: only {len(gens)} decodable generators; not faithful-rebuildable")

    base_a, base_b = gens[0], gens[1]            # two largest landform layers
    detail = gens[len(gens) // 2]                # a mid/high-frequency layer
    for g in (base_a, base_b, detail):
        g.pop("_freq", None)

    erd = next((decode_dev(d) for d in devs if d.cc == "ERD2" and "Capacity" in decode_dev(d)), {})

    nodes = [base_a, base_b, detail]
    nodes.append({"key": "mix", "type": "Blend", "label": "Base blend (CMB2)",
                  "params": {"mode": S("blend"), "factor": F(0.5)}})
    nodes.append({"key": "addD", "type": "Blend", "label": "Add detail (CMB2)",
                  "params": {"mode": S("add"), "factor": F(0.25)}})
    conns = [
        {"from": f"{base_a['key']}.output", "to": "mix.a"},
        {"from": f"{base_b['key']}.output", "to": "mix.b"},
        {"from": "mix.output", "to": "addD.a"},
        {"from": f"{detail['key']}.output", "to": "addD.b"},
    ]
    src = "addD"

    if erd:
        nodes.append({"key": "erode", "type": "HydraulicErosion", "label": "Erosion (ERD2 real params)",
                      "params": {
                          "iterations": U(150000),
                          "erosion_rate": F(clamp(1.0 - erd.get("Hardness", 0.5), 0.02, 0.6)),
                          "deposition_rate": F(0.03),
                          "capacity_factor": F(clamp(0.5 + erd.get("Capacity", 0.3) * 15.5, 0.5, 16.0)),
                          "river_depth": F(clamp(erd.get("River Depth", 0.0), 0.0, 1.0)),
                          "seed": U(int(erd.get("Seed", 0)) & 0xFFFFFFFF),
                      }})
        conns.append({"from": f"{src}.output", "to": "erode.input"})
        src = "erode"

    nodes.append({"key": "clamp", "type": "Clamp", "label": "Clamp (CLMP)",
                  "params": {"mode": S("normalize"), "min": F(0.0), "max": F(1.0)}})
    conns.append({"from": f"{src}.output", "to": "clamp.input"})
    src = "clamp"

    nodes += [
        {"key": "slope", "type": "SlopeMap", "label": "Slope (SSEL src)", "params": {}},
        {"key": "tex", "type": "AutoTexture", "label": "Auto texture", "params": {"biome": S("temperate"), "slope_blend": F(1.0)}},
        {"key": "nrm", "type": "NormalMap", "label": "Normal map", "params": {"strength": F(1.0)}},
        {"key": "fc", "type": "FinalComposition", "label": "Output (OUTP)", "params": {}},
    ]
    conns += [
        {"from": f"{src}.output", "to": "slope.input"},
        {"from": f"{src}.output", "to": "tex.input"},
        {"from": "slope.output", "to": "tex.slope"},
        {"from": f"{src}.output", "to": "nrm.input"},
        {"from": f"{src}.output", "to": "fc.heightmap"},
        {"from": "tex.output", "to": "fc.texture"},
        {"from": "nrm.output", "to": "fc.normalmap"},
    ]

    desc = (f"Param-faithful bar recreation of WM map '{mapname}'. {len(gens)} APRL "
            f"generators decoded; base/detail use their real Scale/Persistence/Seed/"
            f"Steepness. Erosion from real ERD2 params. Topology approximated (WM "
            f"wiring is port-handle indirection, not recovered). Author {meta['author']}.")
    return {
        "name": mapname, "author": meta["author"], "description": desc,
        "nodes": nodes, "connections": conns,
        "output": {"width": 513, "height": 513, "map_settings": {
            "min_height": -64.0, "max_height": 512.0,
            "lighting": {"sun_dir": meta.get("sun", [0.47, 0.6, 0.64])},
            "water": {"base_color": [0.4, 0.55, 0.75]},
        }},
    }


if __name__ == "__main__":
    path, out = sys.argv[1], sys.argv[2]
    mapname = sys.argv[3] if len(sys.argv) > 3 else "map"
    world, devs = M.load(path)
    recipe = build(mapname, world, devs)
    with open(out, "w") as f:
        json.dump(recipe, f, indent=2)
    print(f"wrote {out}: {len(recipe['nodes'])} nodes, {len(recipe['connections'])} connections")
    print("description:", recipe["description"])
