#!/usr/bin/env python3
"""Fetch published halo orbits from the JPL catalogue (ROADMAP C2).

C2 is split in two on purpose: reproducing someone else's orbit tests the
integrator alone, with no search machinery of ours in the way. If a published
orbit fails to close, the bug is in C1. Only then do we search for an orbit
ourselves by differential correction.

JPL carries its own mass_ratio (1.215058560962404e-02), which differs in the
8th digit from the one computed off GM in data/horizons/gm.csv
(0.012150584269542). A published initial condition closes only with the same
mu, so mu is stored beside the orbits rather than taken from the other source.

    python3 scripts/fetch_halo_orbits.py
"""

import json
import sys
import urllib.request
from pathlib import Path

API = ("https://ssd-api.jpl.nasa.gov/periodic_orbits.api"
       "?sys=earth-moon&family=halo&libr=2&branch=S")

OUT_DIR = Path("data/jpl_halo")
ORBITS = OUT_DIR / "halo_l2_south.csv"
MU_FILE = OUT_DIR / "mu.txt"

# Catalogue indices picked to spread the stability index: from a nearly
# neutral orbit to one where a perturbation grows 300x per revolution.
PICKS = [0, 383, 767, 1151, 1534]


def main():
    print("fetching %s" % API)
    with urllib.request.urlopen(API, timeout=60) as response:
        payload = json.load(response)

    system = payload["system"]
    fields = payload["fields"]
    rows = payload["data"]

    expected = ["x", "y", "z", "vx", "vy", "vz", "jacobi", "period", "stability"]
    if fields != expected:
        sys.exit("API format changed: fields %s" % fields)

    OUT_DIR.mkdir(parents=True, exist_ok=True)

    MU_FILE.write_text(
        "# Earth-Moon mass ratio per the JPL catalogue.\n"
        "# Does NOT match the one computed off data/horizons/gm.csv in the\n"
        "# 8th digit; published orbits close only with this value.\n"
        "%s\n" % system["mass_ratio"])

    lines = [
        "# Halo orbits about Earth-Moon L2, southern branch.",
        "# Source: NASA/JPL Three-Body Periodic Orbits API, %s"
        % payload["signature"]["source"],
        "# Catalogue holds %s orbits; %d here, picked to spread stability."
        % (payload["count"], len(PICKS)),
        "# Dimensionless units. mu is in mu.txt alongside.",
        "# index,x,y,z,vx,vy,vz,jacobi,period,stability",
    ]

    for i in PICKS:
        if i >= len(rows):
            sys.exit("index %d outside catalogue (%d orbits)" % (i, len(rows)))
        values = [str(v).strip() for v in rows[i]]
        lines.append("%d,%s" % (i, ",".join(values)))

    ORBITS.write_text("\n".join(lines) + "\n")

    print("wrote %s (%d orbits) and %s" % (ORBITS, len(PICKS), MU_FILE))
    print("  mu = %s" % system["mass_ratio"])
    for i in PICKS:
        r = rows[i]
        print("  index %-5d period %-22s stability %s" % (i, r[7], r[8]))


if __name__ == "__main__":
    main()
