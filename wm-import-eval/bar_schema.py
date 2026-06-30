#!/usr/bin/env python3
"""bar-editor node-graph schema, transcribed from crates/bar-graph/src/node.rs
(default_ports) and defaults.rs (default_params). Used to validate generated
recipes without compiling bar-cli (rustc is absent on this machine).

PORTS[node_type] = (input_port_names, output_port_names)
"""

PORTS = {
    "PerlinNoise":   (["control"], ["output"]),
    "SimplexNoise":  (["control"], ["output"]),
    "WorleyNoise":   (["control"], ["output"]),
    "RidgedNoise":   (["control"], ["output"]),
    "Constant":      ([], ["output"]),
    "HydraulicErosion": (["input", "control", "mask"], ["output", "flow", "wear", "deposit"]),
    "ThermalErosion": (["input", "control", "mask"], ["output"]),
    "Blur":          (["input", "control", "mask"], ["output"]),
    "Clamp":         (["input", "control", "mask"], ["output"]),
    "Terrace":       (["input", "control", "mask"], ["output"]),
    "Sharpen":       (["input", "control", "mask"], ["output"]),
    "Invert":        (["input", "mask"], ["output"]),
    "Mirror":        (["input", "mask"], ["output"]),
    "Blend":         (["a", "b", "control", "mask"], ["output"]),
    "Add":           (["a", "b", "mask"], ["output"]),
    "Subtract":      (["a", "b", "mask"], ["output"]),
    "Multiply":      (["a", "b", "mask"], ["output"]),
    "Max":           (["a", "b", "mask"], ["output"]),
    "Min":           (["a", "b", "mask"], ["output"]),
    "SlopeMap":      (["input", "control"], ["output"]),
    "HeightSelect":  (["input", "control"], ["output"]),
    "TerrainSplat":  (["slope", "band0", "band1", "band2", "control", "mask"], ["output"]),
    "AutoTexture":   (["input", "slope", "control", "mask"], ["output"]),
    "RockSoil":      (["input", "slope", "mask"], ["output"]),
    "Vegetation":    (["input", "slope", "mask"], ["output"]),
    "LayerBlend":    (["base", "overlay", "distribution"], ["output"]),
    "TextureWeightmap": (["texture_0", "texture_1", "texture_2", "texture_3",
                          "texture_4", "texture_5", "texture_6", "texture_7"], ["output"]),
    "ColorRamp":     (["input", "mask"], ["output"]),
    "NormalMap":     (["input", "mask"], ["output"]),
    "GrassMap":      (["input", "slope", "control", "density", "mask"], ["output"]),
    "SpecularMap":   (["input", "slope", "control", "mask"], ["output"]),
    "Mask":          (["input", "control"], ["mask"]),
    "PaintedHeightmap": ([], ["output"]),
    "PaintedTexture": ([], ["output"]),
    "ImportedTexture": ([], ["output"]),
    "MaskThreshold": (["input", "control"], ["output"]),
    "MaskApply":     (["input", "background", "mask"], ["output"]),
    "Curve":         (["input", "control", "mask"], ["output"]),
    "FileInput":     ([], ["output"]),
    "Voronoi":       (["control"], ["output"]),
    "Gradient":      (["control"], ["output"]),
    "Normalize":     (["input", "mask"], ["output"]),
    "BiasGain":      (["input", "control", "mask"], ["output"]),
    "Displacement":  (["input", "displacement", "control", "mask"], ["output"]),
    "FlowSelect":    (["input"], ["output"]),
    "SelectConvexity": (["input"], ["output"]),
    "SlopeSelect":   (["input", "control"], ["output"]),
    "Layout":        (["mask"], ["output"]),
    "Transform":     (["input", "mask"], ["output"]),
    "Warp":          (["input", "warp_x", "warp_y"], ["output"]),
    "Stratify":      (["input", "mask"], ["output"]),
    "MaskExpand":    (["input"], ["output"]),
    "MaskShrink":    (["input"], ["output"]),
    "SelectAspect":  (["input"], ["output"]),
    "MaskSelect":    (["a", "b", "mask"], ["output"]),
    "FinalComposition": (["heightmap", "texture", "normalmap", "metalmap",
                          "typemap", "grassmap", "specular", "files"], []),
    "FileReference": ([], ["file"]),
    "PassThrough":   ([], ["files"]),
    "SubgraphInput": (["value"], ["value"]),
    "SubgraphOutput": (["value"], ["value"]),
}

VALID_PARAM_TAGS = {"Float", "Int", "UInt", "Bool", "String", "Vec2", "Spline"}


def validate_recipe(recipe):
    """Return a list of error strings (empty == valid against the schema)."""
    errs = []
    keys = {}
    for n in recipe.get("nodes", []):
        k = n.get("key")
        t = n.get("type")
        if k in keys:
            errs.append(f"duplicate node key '{k}'")
        keys[k] = t
        if t not in PORTS:
            errs.append(f"node '{k}': unknown type '{t}'")
        for pk, pv in (n.get("params") or {}).items():
            if not isinstance(pv, dict) or len(pv) != 1:
                errs.append(f"node '{k}' param '{pk}': bad ParamValue {pv!r}")
                continue
            tag = next(iter(pv))
            if tag not in VALID_PARAM_TAGS:
                errs.append(f"node '{k}' param '{pk}': bad tag '{tag}'")

    fc = [k for k, t in keys.items() if t == "FinalComposition"]
    if len(fc) != 1:
        errs.append(f"need exactly one FinalComposition node, found {len(fc)}")

    for c in recipe.get("connections", []):
        for end, side in ((c.get("from"), "out"), (c.get("to"), "in")):
            if not end or "." not in end:
                errs.append(f"connection endpoint malformed: {end!r}")
                continue
            nk, port = end.rsplit(".", 1)
            if nk not in keys:
                errs.append(f"connection references unknown node '{nk}'")
                continue
            t = keys[nk]
            if t not in PORTS:
                continue
            allowed = PORTS[t][1] if side == "out" else PORTS[t][0]
            if port not in allowed:
                errs.append(f"connection {end!r}: '{t}' has no {side} port '{port}' (have {allowed})")

    out = recipe.get("output", {})
    if not isinstance(out.get("width"), int) or not isinstance(out.get("height"), int):
        errs.append("output.width/height must be integers")
    return errs


if __name__ == "__main__":
    import sys
    import json
    for p in sys.argv[1:]:
        with open(p) as f:
            r = json.load(f)
        e = validate_recipe(r)
        if e:
            print(f"FAIL {p}")
            for x in e:
                print("   -", x)
        else:
            print(f"OK   {p}  ({len(r['nodes'])} nodes, {len(r['connections'])} connections)")
