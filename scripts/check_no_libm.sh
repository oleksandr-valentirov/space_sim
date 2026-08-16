#!/bin/sh
# The "libm police": forbids libm calls in the deterministic zone of the core.
#
# sin/cos/exp/pow/atan2 are not bit-identical across platforms, nor even across
# libc versions, so the integration loop forbids them (PROJECT.md section 4).
# An invariant held up by discipline alone eventually breaks; this script makes
# it automatically checkable.
#
# sqrt is allowed: IEEE-754 requires correct rounding, so it is the same
# everywhere.
#
# The deterministic zone is the top-level object files build/core/*.o.
# Subdirectories (core/planning: Lambert, porkchop) are deliberately NOT
# checked: planning lies outside the determinism boundary, libm is fine there.

set -eu

OBJ_DIR="${1:-build/core}"

DENY='^(sin|cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|asinh|acosh|atanh|exp|exp2|expm1|log|log2|log10|log1p|pow|cbrt|hypot|fmod|remainder|erf|erfc|lgamma|tgamma|sincos)f?l?$'

objs=$(find "$OBJ_DIR" -maxdepth 1 -name '*.o' 2>/dev/null || true)
if [ -z "$objs" ]; then
    echo "check-libm: ERROR -- no object files found in $OBJ_DIR" >&2
    echo "  (an empty check would silently 'pass', so this counts as failure)" >&2
    exit 1
fi

# nm -P is the portable format: "symbol type value size".
# sed strips the glibc version (sin@GLIBC_2.2.5) and the macOS leading
# underscore.
symbols=$(
    # shellcheck disable=SC2086
    nm -P -u $objs 2>/dev/null \
        | awk '{print $1}' \
        | sed -e 's/@.*//' -e 's/^_//' \
        | sort -u
) || {
    echo "check-libm: nm unavailable or does not support -P." >&2
    echo "  Fallback from ROADMAP A2 is a source-level check." >&2
    exit 1
}

found=$(printf '%s\n' "$symbols" | grep -E "$DENY" || true)

if [ -n "$found" ]; then
    echo "check-libm: FAILED -- libm in the deterministic zone:" >&2
    printf '  %s\n' $found >&2
    echo "" >&2
    echo "  Allowed: + - * / and sqrt. Workarounds are in PROJECT.md section 4:" >&2
    echo "    harmonics -- Pines recursions (no trigonometry)" >&2
    echo "    body rotation -- Chebyshev polynomials from the asset" >&2
    echo "    atmosphere -- density table with polynomial interpolation" >&2
    exit 1
fi

count=$(printf '%s\n' "$symbols" | grep -c . || true)
echo "check-libm: clean (undefined symbols checked: $count)"
