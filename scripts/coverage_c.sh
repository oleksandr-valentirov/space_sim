#!/bin/sh
# Line coverage of the C core, aggregated from gcov. Driven by `make coverage`,
# which builds the instrumented tree and runs the unit tests first.
#
# Usage: sh scripts/coverage_c.sh <cov-dir> <source.c>...
#
# Writes the percentage to <cov-dir>/percent.txt (one number, for the README
# badge) and a human line to stdout.
#
# Why gcov -n rather than the .gcov files: -n writes nothing, so there is no
# output tree to place, clean or gitignore, and no per-line parsing. The
# percentage stays EXACT anyway -- gcov prints two decimals, so
# round(pct * n / 100) recovers the integer count for any file under 10000
# lines, and ours are hundreds.
#
# Only .c under core/ counts, and not core/test: the badge is about the
# library. A test file's own lines mostly measure how much of the test ran,
# which is always nearly all of it, so including them would inflate the number
# by a constant that says nothing.

set -eu

# awk's printf honours the locale, so on a uk_UA machine "%.1f" prints "94,0"
# -- which is a fine number to read and an invalid one to put in the badge
# JSON. The Makefile pins LC_ALL for sed for the same class of reason.
LC_ALL=C
export LC_ALL

COV_DIR="${1:?usage: coverage_c.sh <cov-dir> <source.c>...}"
shift

if [ $# -eq 0 ]; then
    echo "coverage: ERROR -- no sources given" >&2
    exit 1
fi

GCOV="${GCOV:-gcov}"

for src in "$@"; do
    # -o points at the directory holding the .gcno/.gcda pair, which is where
    # the object file was written; the source path is given relative to the
    # repository root, exactly as it was compiled.
    "$GCOV" -n -o "$COV_DIR/$(dirname "$src")" "$src" 2>/dev/null || true
done | awk -v out="$COV_DIR/percent.txt" '
    # "File \x27core/accel.c\x27" -- the name runs from column 7 to the
    # character before the closing quote. Taken by offset rather than by
    # pattern so this program needs no quote literal of its own.
    /^File / { f = substr($0, 7, length($0) - 7); next }

    # "Lines executed:86.05% of 172"
    /^Lines executed:/ {
        if (f ~ /\.c$/ && f ~ /^core\// && f !~ /^core\/test\// && !(f in seen)) {
            seen[f] = 1
            split($0, a, ":")
            split(a[2], b, "%")
            n = $NF + 0
            cov += int(b[1] * n / 100 + 0.5)
            tot += n
        }
        next
    }

    END {
        # An empty report would otherwise print "100%" or "0%" and look like an
        # answer. Missing .gcda is the likely cause, and that is a broken
        # measurement, not a low one.
        if (tot == 0) {
            print "coverage: ERROR -- gcov produced no data for any source" > "/dev/stderr"
            exit 1
        }
        pct = 100 * cov / tot
        printf "%.1f\n", pct > out
        printf "\ncoverage: %d of %d lines in the C sources, %.1f%%\n", cov, tot, pct
    }
'
