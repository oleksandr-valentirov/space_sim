#!/bin/sh
# Writes one shields.io "endpoint" JSON file -- the source of a README badge.
#
# Usage: sh scripts/badge_json.sh <label> <percent> <out.json>
#
# The endpoint schema is shields.io's own: it fetches this file and renders the
# badge from it. That is the whole reason no coverage service is involved --
# the number is computed here, hosted in the repository's own `badges` branch,
# and shields only draws it.
#
# cacheSeconds is the shortest shields honours (300). Raw GitHub content has a
# CDN cache of its own on top, so a fresh number takes a few minutes to appear;
# that is fine for a badge and there is nothing to tune.

set -eu

LABEL="${1:?usage: badge_json.sh <label> <percent> <out.json>}"
PERCENT="${2:?missing percent}"
OUT="${3:?missing output path}"

# Locale-proof: a comma decimal separator would be valid on this machine and
# invalid JSON. The producers already pin LC_ALL, this is the second line of
# defence for a hand-run invocation.
case "$PERCENT" in
    *,*) PERCENT=$(printf '%s' "$PERCENT" | tr ',' '.') ;;
esac

# Thresholds are the conventional shields ladder, not a measurement. They exist
# so the colour carries information at a glance; nothing in the build reads
# them, and no gate fires at any of them.
whole=${PERCENT%%.*}
if   [ "$whole" -ge 90 ]; then colour=brightgreen
elif [ "$whole" -ge 80 ]; then colour=green
elif [ "$whole" -ge 70 ]; then colour=yellowgreen
elif [ "$whole" -ge 60 ]; then colour=yellow
elif [ "$whole" -ge 50 ]; then colour=orange
else                           colour=red
fi

mkdir -p "$(dirname "$OUT")"
cat > "$OUT" <<EOF
{
  "schemaVersion": 1,
  "label": "$LABEL",
  "message": "$PERCENT%",
  "color": "$colour",
  "cacheSeconds": 300
}
EOF

echo "$OUT: $LABEL $PERCENT% ($colour)"
