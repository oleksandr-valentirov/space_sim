#!/bin/sh
# Download reference data from JPL Horizons (ROADMAP B1).
#
# This data is the oracle our own integrator is measured against. Writing an
# integrator with nothing to compare against is the most expensive way to move,
# so this step comes before B3, not after it.
#
# The result is committed: the data is static and need not be fetched again.
# The script exists for reproducibility and to document the query, not for
# regular runs.
#
# IMPORTANT -- the query parameters are part of the data contract:
#   CENTER='500@0'   solar system barycentre
#   REF_PLANE='FRAME' + REF_SYSTEM='ICRF'   equatorial ICRF, NOT ecliptic
#   OUT_UNITS='KM-S' km and km/s -- converted to metres on import (vec3.h)
#   VEC_CORR='NONE'  geometric vectors, no light-time or aberration
#   Time is JDTDB (barycentric dynamical), not UTC: no leap seconds.
#
# Changing any of these makes the data incompatible with what came before.
# This is where most discrepancies in numerical work are born -- not in the
# physics.

set -eu

OUT_DIR="${1:-data/horizons}"
API="https://ssd.jpl.nasa.gov/api/horizons.api"

START="2000-01-01 12:00"
STOP="2010-01-01 12:00"
STEP="30d"

# id:name:obj_id
#   id     -- for vectors
#   obj_id -- for body parameters; differs for barycentres, because a
#             barycentre is a dynamical point and has no published GM
#
# All the major solar system bodies.
#
# The temptation to take only the Sun, Earth and Moon exists, but B5 measured
# its cost: a subsystem of a few bodies carries only part of the solar system's
# momentum, so its shared barycentre drifts linearly through the SSB frame --
# 4.96e9 m over 10 years for three bodies. That drift would bake into the
# ephemeris asset. The cure is a complete set of bodies, not a better
# integrator.
BODIES="10:sun:10 199:mercury:199 299:venus:299 399:earth:399 301:moon:301 \
4:mars_bary:499 5:jupiter_bary:599 6:saturn_bary:699 7:uranus_bary:799 \
8:neptune_bary:899"

mkdir -p "$OUT_DIR"

fetch_vectors() {
    id="$1"; name="$2"
    echo "  vectors: $name ($id)"
    curl -sS -G "$API" \
        --data-urlencode "format=text" \
        --data-urlencode "COMMAND='$id'" \
        --data-urlencode "OBJ_DATA='NO'" \
        --data-urlencode "MAKE_EPHEM='YES'" \
        --data-urlencode "EPHEM_TYPE='VECTORS'" \
        --data-urlencode "CENTER='500@0'" \
        --data-urlencode "START_TIME='$START'" \
        --data-urlencode "STOP_TIME='$STOP'" \
        --data-urlencode "STEP_SIZE='$STEP'" \
        --data-urlencode "REF_PLANE='FRAME'" \
        --data-urlencode "REF_SYSTEM='ICRF'" \
        --data-urlencode "OUT_UNITS='KM-S'" \
        --data-urlencode "VEC_TABLE='2'" \
        --data-urlencode "VEC_LABELS='NO'" \
        --data-urlencode "VEC_CORR='NONE'" \
        --data-urlencode "CSV_FORMAT='YES'" \
        > "$OUT_DIR/.raw_$name.txt"

    if ! grep -q '\$\$SOE' "$OUT_DIR/.raw_$name.txt"; then
        echo "ERROR: Horizons returned no table for $name ($id)" >&2
        head -20 "$OUT_DIR/.raw_$name.txt" >&2
        exit 1
    fi

    {
        echo "# JPL Horizons, $name (COMMAND=$id)"
        echo "# center=SSB(500@0) frame=ICRF/FRAME units=KM-S corr=NONE"
        echo "# jdtdb,x_km,y_km,z_km,vx_kms,vy_kms,vz_kms"
        sed -n '/\$\$SOE/,/\$\$EOE/p' "$OUT_DIR/.raw_$name.txt" \
            | sed -e '/\$\$SOE/d' -e '/\$\$EOE/d' \
            | awk -F', *' '{printf "%s,%s,%s,%s,%s,%s,%s\n", $1,$3,$4,$5,$6,$7,$8}'
    } > "$OUT_DIR/vec_$name.csv"

    rm -f "$OUT_DIR/.raw_$name.txt"
}

# Gravitational parameters come from the same source as the vectors, not from
# memory or a textbook: otherwise the force and the reference use different GM,
# and the discrepancy gets blamed on the integrator.
fetch_object_data() {
    id="$1"; name="$2"
    echo "  body parameters: $name ($id)"
    curl -sS -G "$API" \
        --data-urlencode "format=text" \
        --data-urlencode "COMMAND='$id'" \
        --data-urlencode "OBJ_DATA='YES'" \
        --data-urlencode "MAKE_EPHEM='NO'" \
        | grep -v '^Ephemeris / API_USER' \
        > "$OUT_DIR/obj_$name.txt"
}

echo "Fetching from JPL Horizons -> $OUT_DIR"
echo "  interval: $START .. $STOP, step $STEP"

for entry in $BODIES; do
    id=$(echo "$entry" | cut -d: -f1)
    name=$(echo "$entry" | cut -d: -f2)
    obj_id=$(echo "$entry" | cut -d: -f3)
    fetch_vectors "$id" "$name"
    fetch_object_data "$obj_id" "$name"
done

# Extract GM in machine-readable form.
#
# We match the assignment "GM ... = number", not the line it occurs on. The
# reason is concrete: for the Moon, GM sits mid-line next to the radius
#   "Radius (IAU), km = 1737.4    GM, km^3/s^2 = 4902.800066"
# and any "take the number after the first =" would yield the radius.
#
# The "GM 1-sigma" line is filtered by the shape of the pattern itself: GM must
# be followed by a unit, not by "1-sigma".
{
    echo "# Gravitational parameters from JPL Horizons, km^3/s^2."
    echo "# Source: obj_<name>.txt in this same directory."
    echo "# NOTE: for mars_bary and jupiter_bary this is the GM of the PLANET,"
    echo "# not of the system. See README.md, the section on barycentre GM."
    echo "# name,gm_km3_s2"
    for entry in $BODIES; do
        name=$(echo "$entry" | cut -d: -f2)
        gm=$(grep -oiE 'GM[ ,]*\(?km\^3/s\^2\)?[ ,]*=[[:space:]]*[0-9.]+' \
                "$OUT_DIR/obj_$name.txt" \
             | head -1 \
             | sed -E 's/.*=[[:space:]]*//')
        if [ -z "$gm" ]; then
            echo "ERROR: no GM found for $name" >&2
            exit 1
        fi
        echo "$name,$gm"
    done
} > "$OUT_DIR/gm.csv"

echo "Done. Rows per table:"
for entry in $BODIES; do
    name=$(echo "$entry" | cut -d: -f2)
    printf "  %-14s %s\n" "$name" "$(grep -vc '^#' "$OUT_DIR/vec_$name.csv")"
done
echo "GM:"
grep -v '^#' "$OUT_DIR/gm.csv" | sed 's/^/  /'
