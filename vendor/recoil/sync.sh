#!/usr/bin/env bash
# Refresh the vendored Recoil shader copies from a local Recoil clone.
#
# Usage: bash vendor/recoil/sync.sh [path-to-recoil-clone]
#
# Default source path: ~/Projects/bar-recoil
#
# Run from the OM repo root. After this script writes files, update the
# pinned commit hash in vendor/recoil/UPSTREAM.md to match the source's
# HEAD.

set -euo pipefail

SRC="${1:-${HOME}/Projects/bar-recoil}"
DST_ROOT="vendor/recoil"
SHADER_SRC="${SRC}/cont/base/springcontent/shaders/GLSL"
SHADER_DST="${DST_ROOT}/shaders/GLSL"

if [[ ! -d "${SHADER_SRC}" ]]; then
    echo "error: ${SHADER_SRC} not found" >&2
    echo "       pass the recoil clone path as the first argument" >&2
    exit 1
fi

# Files we actually port. Keep this list narrow; vendoring more than we
# use makes future sync diffs noisier without value.
SHADERS=(
    SMFFragProg.glsl
    SMFVertProg.glsl
    SMFShadingTextureFragProg.glsl
    SMFShadingTextureVertProg.glsl
    SMFBorderVertProg.glsl
    SMFBorderFragProg.glsl
    BumpWaterFS.glsl
    BumpWaterVS.glsl
    ModernSkyFS.glsl
    ModernSkyVS.glsl
    MiniMapFragProg.glsl
    MiniMapVertProg.glsl
)

mkdir -p "${SHADER_DST}"

for f in "${SHADERS[@]}"; do
    cp -v "${SHADER_SRC}/${f}" "${SHADER_DST}/${f}"
done

# License files (cheap; copied each sync so a license update upstream
# can't drift).
cp -v "${SRC}/LICENSE" "${DST_ROOT}/LICENSE"
cp -v "${SRC}/AUTHORS" "${DST_ROOT}/AUTHORS"
cp -v "${SRC}/gpl-2.0.txt" "${DST_ROOT}/GPL-2.0.txt"
cp -v "${SRC}/gpl-3.0.txt" "${DST_ROOT}/GPL-3.0.txt"

echo
echo "Synced from: ${SRC}"
echo "Pinned commit: $(git -C "${SRC}" rev-parse HEAD)"
echo "Update vendor/recoil/UPSTREAM.md if this differs from the recorded hash."
