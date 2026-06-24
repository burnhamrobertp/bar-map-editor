#!/usr/bin/env python3
"""World Machine device (FourCC) -> bar-editor node mapping.

Identities and config surfaces were confirmed from each device's embedded
PAR2 tooltip/enum strings (see tmd_model.py output), not guessed. Status:
  good    = a bar node reproduces the device faithfully enough
  partial = a bar node exists but loses configurability the device exposes
  gap     = bar-editor has no equivalent node/capability
"""

# cc: (wm_name, bar_node, status, note)
MAP = {
    # --- direct / faithful ---
    "APRL": ("Advanced Perlin", "PerlinNoise", "good", "core FBM noise"),
    "BPRL": ("Basic Perlin", "PerlinNoise", "good", ""),
    "CELL": ("Voronoi", "Voronoi", "good", "Scale/Style -> frequency/mode"),
    "CHGT": ("Constant Height", "Constant", "good", ""),
    "BLUR": ("Blur", "Blur", "good", "WM adds Approximate/Precise method enum"),
    "HSEL": ("Select Height", "HeightSelect", "good",
             "[CLOSED] HeightSelect now has falloff_type (linear/smooth) + invert"),
    "BIGA": ("Bias/Gain", "BiasGain", "good", ""),
    "IVRT": ("Invert", "Invert", "good", ""),
    "LAYG": ("Layout Generator", "Layout", "good", "shapes + splines; strong match"),
    "OUTP": ("Output", "FinalComposition", "good", "terminal"),
    "SELF": ("Select by Direction", "SelectAspect", "good", "Heading/Elevation"),
    "CGRD": ("Radial Gradient", "Gradient", "good", "radial mode"),
    "PRNO": ("Normal Map Maker", "NormalMap", "good", ""),
    "NORM": ("Texture Weightmap", "TextureWeightmap", "good", "priority/weighted blend"),
    "SELC": ("Select Convexity", "SelectConvexity", "good", "Exposed/Recessed/Transition"),
    "GRAD": ("Gradient", "Gradient", "good", ""),
    "CURV": ("Curve", "Curve", "good", "transfer curve"),
    "FILI": ("File Input", "FileInput", "good", "external heightmap/image source"),
    "TRCE": ("Terrace", "Terrace", "good", "WM adds Simple/Sharp/smooth method"),
    "FLIP": ("Flip", "Mirror", "good", "H/V flip"),

    # --- partial / lossy ---
    "CMB2": ("Combiner", "Blend (Combine, 11-way mode)", "good",
             "[CLOSED] Blend node is now a universal Combine with an 11-way mode enum "
             "(add/subtract/multiply/divide/average/screen/power/difference/max/min/blend) "
             "+ factor as WM Strength + mask"),
    "MACR": ("Macro", "SubgraphInput/Output", "partial",
             "reusable subgraph w/ exposed knobs + library; bar has subgraph nesting + presets"),
    "CLMP": ("Clamp / Restrict", "Clamp", "good",
             "[CLOSED] Clamp gained mode = clamp/normalize/soft_clip; "
             "only WM's Rescale/Expand 'Type' enum nuance remains"),
    "SSEL": ("Select Slope", "SlopeSelect", "good",
             "[CLOSED] new native SlopeSelect node: slope range in degrees + "
             "falloff + falloff-type + invert"),
    "CHOS": ("Chooser", "MaskSelect", "partial", "2-input select by control"),
    "PRBO": ("Bitmap Output", "FinalComposition(channel)", "partial",
             "WM writes arbitrary named bitmaps (BMP/TIFF/PNG); bar only the fixed BAR channel set"),
    "EXPA": ("Expander", "MaskExpand|MaskShrink", "partial",
             "WM has Expand/Shrink/Open/Close + Gaussian; bar lacks Open/Close compound"),
    "ERD2": ("Erosion", "HydraulicErosion", "partial",
             "bar now exposes 10 CPU params (incl. Capacity, Seed, inertia, "
             "evaporation, gravity, radius, lifetime); still missing Hardness, "
             "River Depth, Method, Multiscale + GPU-path parity"),
    "EWTH": ("Thermal Weathering", "ThermalErosion", "partial",
             "WM: Talus-production/repose/fracture/size/length; bar: iterations+talus_angle"),
    "WARP": ("Displacement", "Warp|Displacement", "partial", "WM: Direction/Edge-Handling/Strength"),
    "C_DG": ("Distortion Generator", "Warp", "partial", "Angle&Power / Vector distortion"),
    "OLVW": ("Overlay / Colorizer", "LayerBlend|ColorRamp", "partial",
             "Mask Colors / Blend-with-Mask / Mask Ambient"),
    "COLG": ("Color Generator", "PaintedTexture|ColorRamp", "partial", "no solid-colour source node"),
    "C_GN": ("Coordinate/Transform Gen", "Transform|Mirror", "partial", "e.g. Rotate 180"),
    "lvls": ("Levels", "BiasGain|Curve", "partial", "black/white-point remap"),
    "HSPL": ("Height Splitter", "HeightSelect(xN)", "partial", "N height bands in one device"),
    "PULL": ("Pull-up / Override", "Max|Min|MaskApply", "partial", "conditional override"),
    "ANOI": ("Add Noise", "PerlinNoise+Add", "partial", "Normal/Additive convenience combine"),
    "MTRN": ("Terrain Transform", "Curve|BiasGain", "partial",
             "named geomorphic presets: Canyonize/Glaciate/Cubic-Midlands/Midland-Plateau; bar has none"),

    # --- gap: no bar equivalent ---
    "EQUA": ("Equation", "Equation", "good", "[CLOSED] native Equation node (evalexpr per-pixel formula a/b/c/d,x,y,h)"),
    "SWCH": ("Switch", "Switch", "good", "[CLOSED] native N-way Switch (runtime-resizable inputs + selector)"),
    "CHKP": ("Checkpoint", "Checkpoint", "good", "[CLOSED] native Checkpoint passthrough (incremental caching deferred)"),
    "PRLM": ("Render / Lightmap", "LightmapBake", "partial",
             "[CLOSED] native LightmapBake (horizon AO + sun ray-march bake, CPU+GPU); "
             "not a full raytraced colour render"),
    "PRCC": ("Channel Splitter", "ChannelSplit", "good", "[CLOSED] native ChannelSplit (Color -> R/G/B/A heightmaps)"),
    "PRCS": ("Channel Combiner", "ChannelMerge", "good", "[CLOSED] native ChannelMerge (R/G/B/A -> Color)"),
    "SELU": ("Select Color", None, "gap", "build a mask by selecting a colour/hue"),
    "S_GN": ("Scalar Generator", "ScalarValue", "good",
             "[CLOSED] native ScalarValue + scalar-wire param binding: a scalar drives any "
             "scalar_bindable node param via the eval.rs fold"),
    "S_AR": ("Scalar Arithmetic", "ScalarMath", "good", "[CLOSED] native ScalarMath (add/sub/mul/div/min/max/avg/power)"),
    "I_GN": ("Integer Generator", "IntValue", "good", "[CLOSED] native IntValue node"),
    "COG2": ("Combiner-of-Channels gen2 (unconfirmed)", None, "gap",
             "colour/channel combiner variant; identity not fully confirmed"),
    "COER": ("Coast Erosion", "CoastErosion", "good",
             "[CLOSED] native CoastErosion (sea level, beach size, inland influence, "
             "underwater smoothing)"),
    "SCVW": ("Scene View", None, "gap",
             "3D scene/material-view device (view-only, no terrain output); n/a in bar"),
}


def lookup(cc):
    return MAP.get(cc, (f"<unknown {cc}>", None, "gap", "unrecognised device type"))


if __name__ == "__main__":
    for st in ("good", "partial", "gap"):
        rows = [(cc, v) for cc, v in MAP.items() if v[2] == st]
        print(f"\n== {st.upper()} ({len(rows)}) ==")
        for cc, (nm, node, _, note) in rows:
            print(f"  {cc}  {nm:28} -> {node}")
