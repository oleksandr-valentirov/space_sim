#!/bin/sh
# Compile Slang shaders to WGSL (ROADMAP F2).
#
# The output IS COMMITTED, on the same architecture as the assets: the cooker
# runs once on the developer's machine and the build ships the result
# (PROJECT.md section 4). Otherwise every engine build, and CI too, would need
# 24 MB of Slang compiler.
#
# Side benefit: the generated WGSL shows up in the diff. When a generic expands
# into the wrong thing, it is visible in review rather than in the frame.
#
# The slang-probe workflow checks the output has not gone stale: it downloads
# slangc, rebuilds and looks for a difference.
#
#   sh scripts/fetch_slang.sh      once
#   sh scripts/build_shaders.sh
#
# Run from the repository root.

set -eu

SLANGC="${SLANGC:-tools/slang/bin/slangc}"
DIR="${1:-engine/shaders}"

if [ ! -x "$SLANGC" ]; then
    echo "missing $SLANGC -- run first: sh scripts/fetch_slang.sh" >&2
    exit 1
fi

found=0
for source in "$DIR"/*.slang; do
    [ -e "$source" ] || continue
    found=1
    out="${source%.slang}.wgsl"
    echo "  $source -> $out"
    "$SLANGC" "$source" -target wgsl -o "$out"

    # A single post-compiler fixup, and not a cosmetic one.
    #
    # Slang prints a bindless texture array as `array<texture_2d<...>>`, which
    # `naga` does not accept at all: a resource array in WGSL has its own
    # keyword, `binding_array`, and without it the module fails
    # `create_shader_module` with "Base type for the array is invalid".
    #
    # Same class of divergence that already forced WGSL over SPIR-V (capability
    # DrawParameters, ROADMAP F2): two tools read one spec slightly
    # differently. The substitution is deliberately narrow -- it touches only
    # `var ... : array<texture_...>` declarations, exactly the construct that
    # does not exist in WGSL in that form.
    sed -i.bak -E \
        's/(var [A-Za-z0-9_]+ : )array<(texture_[a-z0-9_]+<[^>]*>)>/\1binding_array<\2>/' \
        "$out"
    rm -f "$out.bak"

    # And if an array did appear, the extension switch goes on the first line.
    # `binding_array` is a wgpu extension, not the WGSL spec itself, so without
    # `enable` the module fails saying exactly what is missing. Added here
    # rather than by hand in the .wgsl: nobody edits a generated file.
    if grep -q "binding_array<" "$out"; then
        printf 'enable wgpu_binding_array;\n\n%s' "$(cat "$out")" > "$out.tmp"
        mv "$out.tmp" "$out"
    fi
done

if [ "$found" -eq 0 ]; then
    echo "no .slang in $DIR -- nothing to do, which is suspicious" >&2
    exit 1
fi

echo "Done. Compiler version: $(cat tools/slang/VERSION 2>/dev/null || echo unknown)"
