#!/usr/bin/env python3
"""Per-parameter gap: decode every instance of the high-value device types
across both maps, aggregate the values map-makers actually use, and line each
WM parameter up against its bar-editor equivalent (or mark it a gap).

Emits a markdown table to stdout (redirect into PARAM_GAPS.md)."""
import statistics
import tmd_model as M
import par2_decode as P
import parmgroup as PG

# Newer-format (Aethermoor) param names aliased to the classic (ATG2) names so
# both WM serialization families land in the same bucket.
ALIASES = {
    "Feature Scale": "Scale", "Middle elevation": "Elevation",
    "Lowest": "Minimum", "Highest": "Maximum", "Lead-in octave": "Multiscale Lead-in",
    "Shapeguide lead-in level": "Shapeguide Power", "Distortion guide level": "Distortion Power",
    "Persistence guide level": "Persistence Guide", "Activity": "Multiscale Power",
}


def decode_any(dev):
    """Decode params regardless of WM format family: flat PAR2 (ATG2) or the
    self-describing ParmGroup tree (Aethermoor / newer builds)."""
    params, _ = P.decode(dev.par2())
    if not params:
        params = PG.decode(dev.params)
    return [(ALIASES.get(n, n), v) for n, v in params]

import glob
import os

FILES = sorted(glob.glob(os.path.expanduser("~/Downloads/*.tmd")))

# WM param name -> (bar node.param, note). None bar target == gap.
# Keyed by device FourCC. bar param sets are from crates/bar-graph/src/defaults.rs.
PMAP = {
    "APRL": ("PerlinNoise", {
        "Scale": ("frequency", "inverse sense (WM scale vs bar frequency)"),
        "Persistence": ("persistence", ""),
        "Lacunarity": ("lacunarity", ""),
        "Octaves": ("octaves", ""),
        "Seed": ("seed", ""),
        "Style": (None, "8 noise styles (Basic/Ridged/Billowy/Smooth/Sharp/Flat/Terraced/+presets); bar has 5 'character' presets on Perlin only"),
        "Steepness": (None, "per-noise steepness shaping"),
        "Elevation": (None, "vertical placement"),
        "Offset": (None, ""),
        "Gain": (None, ""),
        "Shapeguide Power": (None, "shape-guide input weighting"),
        "Distortion Power": (None, "built-in domain distortion (bar: separate Warp node)"),
        "Persistence Guide": (None, "spatially-varying persistence"),
        "Multiscale Power": (None, "multiscale synthesis"),
        "Multiscale Lead-in": (None, "multiscale synthesis"),
        "Multiscale Type": (None, "multiscale synthesis"),
        "Specify Height Range": (None, "output range remap built into the generator"),
        "Minimum": (None, "output min (bar: separate Clamp)"),
        "Maximum": (None, "output max (bar: separate Clamp)"),
    }),
    "CMB2": ("Combine (Blend node)", {
        "Method": ("Blend.mode", "[CLOSED] now an 11-way mode enum on the Combine node (add/subtract/multiply/divide/average/screen/power/difference/max/min/blend)"),
        "Strength": ("Blend.factor", "blend amount; factor lerps a->op(a,b)"),
    }),
    "CLMP": ("Clamp", {
        "Range1": ("min", ""),
        "Range2": ("max", ""),
        "Type": (None, "Rescale/Expand enum -- partially covered by mode"),
        "Normalize": ("Clamp.mode", "[CLOSED] mode=normalize"),
        "Soft Clipping": ("Clamp.mode", "[CLOSED] mode=soft_clip"),
    }),
    "BLUR": ("Blur", {
        "Radius": ("radius", "WM radius is normalized 0..1; bar radius is in pixels"),
        "Blur method": (None, "Approximate (fast) / Precise enum"),
        "Specify radius in": (None, "radius unit: percent vs meters"),
        "Direction": (None, "directional / motion blur angle; bar blur is isotropic"),
        "Isolate masked areas": ("mask", "mask input (present)"),
    }),
    "ERD2": ("HydraulicErosion", {
        "Amount": ("iterations", "WM 'Amount' (duration) loosely ~ bar iterations"),
        "Hardness": (None, "rock hardness / resistance"),
        "Capacity": ("capacity_factor", "[CLOSED] surfaced"),
        "Filter Type": (None, "erosion filter kernel"),
        "Filter Strength": (None, ""),
        "Method": (None, "erosion algorithm variant"),
        "Seed": ("seed", "[CLOSED] surfaced"),
        "River Depth": (None, "channel incision depth"),
        "River Bias": (None, ""),
        "Multiscale Enable": (None, "multi-resolution erosion"),
        "Multiscale Bias": (None, ""),
        "Multiscale Synthesis": (None, ""),
        "Scale Independence": (None, "resolution-independent result"),
        "Preserve Edges": (None, ""),
        "Use Original Mask Style": (None, ""),
        "Use Active Masking": ("mask", "mask input (present)"),
        "Hardness Map Behavior": (None, "per-pixel hardness input"),
        # bar-only: erosion_rate, deposition_rate (no WM equivalent name)
    }),
    "HSEL": ("HeightSelect", {
        "Minimum": ("low", ""),
        "Maximum": ("high", ""),
        "Falloff": ("falloff", ""),
        "Falloff type": ("falloff_type", "[CLOSED] linear/smooth"),
        "Invert Selection": ("invert", "[CLOSED] invert toggle added"),
    }),
    "SSEL": ("SlopeSelect (new native node)", {
        "Minimum": ("min_slope", "[CLOSED] degrees"),
        "Maximum": ("max_slope", "[CLOSED] degrees"),
        "Falloff": ("falloff", "[CLOSED] degrees"),
        "Falloff type": ("falloff_type", "[CLOSED] linear/smooth"),
        "Invert Selection": ("invert", "[CLOSED]"),
    }),
    "TRCE": ("Terrace", {
        "Number of Terraces": ("step_count", ""),
        "Terrace Method": (None, "Simple/Sharp/Smooth edge enum"),
        "Terrace Shape": ("smoothing", "loosely ~ smoothing"),
        "Terrace Layering": (None, "layering mode"),
    }),
    "BIGA": ("BiasGain", {
        "Bias": ("bias", ""),
        "Gain": ("gain", ""),
    }),
    "EWTH": ("ThermalErosion", {
        "Talus Repose Angle": ("talus_angle", ""),
        "Talus Production": (None, "material production rate"),
        "Fracture Size": (None, ""),
        "Talus Size": (None, ""),
        "Simulation Length": ("iterations", "loosely ~ iterations"),
    }),
    "WARP": ("Warp / Displacement", {
        "Strength": ("strength", ""),
        "Direction": (None, "displacement direction/angle"),
        "Edge Handling": (None, "wrap/clamp at edges"),
    }),
}


def aggregate():
    acc = {}  # cc -> {param: [values]}
    counts = {}  # cc -> instance count
    for f in FILES:
        _, devs = M.load(f)
        for d in devs:
            if d.cc not in PMAP:
                continue
            params = decode_any(d)
            if not params:
                continue
            counts[d.cc] = counts.get(d.cc, 0) + 1
            bucket = acc.setdefault(d.cc, {})
            for name, val in params:
                if isinstance(val, (int, float)):
                    bucket.setdefault(name, []).append(val)
    return acc, counts


def fmt_vals(vals):
    nums = [v for v in vals if isinstance(v, (int, float))]
    if not nums:
        return "-"
    lo, hi = min(nums), max(nums)
    med = statistics.median(nums)
    if lo == hi:
        return f"{lo:g}"
    return f"{lo:g}..{hi:g} (med {med:g})"


def main():
    acc, counts = aggregate()
    print("# Per-parameter WM -> bar-editor gap\n")
    print(f"Values aggregated across every device instance in all {len(FILES)} "
          "corpus maps (both WM param formats decoded). 'mapped' = a bar param "
          "carries it; blank bar column = gap.\n")
    for cc, (barnode, pm) in PMAP.items():
        n = counts.get(cc, 0)
        mapped = sum(1 for _, (b, _) in pm.items() if b)
        total = len(pm)
        print(f"\n## {cc} -> {barnode}")
        print(f"_{n} instances; {mapped}/{total} WM params have a bar equivalent_\n")
        print("| WM param | typical value | bar param | note |")
        print("|----------|---------------|-----------|------|")
        bucket = acc.get(cc, {})
        for wname, (btarget, note) in pm.items():
            vals = fmt_vals(bucket.get(wname, []))
            bcol = btarget if btarget else "**gap**"
            print(f"| {wname} | {vals} | {bcol} | {note} |")


if __name__ == "__main__":
    main()
