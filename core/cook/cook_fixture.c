/* Cook the committed ephemeris fixture (ROADMAP C5).
 *
 * The determinism scenarios have to reach the runtime's asset-reading path,
 * and that path needs an asset. It cannot cook one for itself: a scenario
 * links against libcore.a alone, without libm, precisely so that any
 * trigonometry leaking into the runtime fails to link - and Chebyshev fitting
 * is trigonometry.
 *
 * So the asset is committed, and this is what produces it. That is not a
 * workaround, it is the architecture: PROJECT.md section 4 has the cooker run
 * once on the developer's machine and the result ship. A scenario loading a
 * shipped asset is doing exactly what the game does.
 *
 * Which also means this program's output must NOT be regenerated per platform
 * before comparing hashes. Two machines fitting their own coefficients would
 * disagree in the last bits through cos(), and the cross-platform check would
 * fail on the cooker rather than on the runtime it is meant to test. Cook
 * once, commit, compare.
 *
 *   make cook     regenerate data/fixture/earth_moon.eph
 *
 * Run from the repository root. */

#include "eph_build.h"
#include "refdata.h"

#include <stdio.h>
#include <string.h>

#define MAX_SAMPLES 8
#define DAY 86400.0

/* Every major body, as ROADMAP B5 concluded it must be: with a subset, a
 * linear drift of the subsystem's barycentre is baked into the asset. */
/* Radii and the solar flux are cited, never invented - the rule
 * data/horizons/README.md states and K2 followed for J2. Each number is the
 * volumetric mean radius from that body's own already-committed Horizons
 * page, in metres:
 *
 *   sun 695700 km (also the IAU 2015 solar radius), mercury 2439.4,
 *   venus 6051.84, earth 6371.01, moon 1737.53, mars 3389.92,
 *   jupiter 69911, saturn 58232, uranus 25362, neptune 24624.
 *
 * Mean rather than equatorial, deliberately: this radius draws a shadow, and
 * a shadow is cast by the whole disc. It is a different number from the
 * reference radius of the harmonic field below (6378137 m, equatorial), which
 * is a scale in an expansion rather than a size, and the two must not be
 * confused into one field.
 *
 * The outer four bodies are barycentres, so this is the planet's radius
 * standing in for the system's. The error is the planet's offset from its own
 * barycentre - largest for Jupiter, and there it is under a thousandth of the
 * radius itself.
 *
 * The Sun's flux is the "Solar constant (1 AU)" line of obj_sun.txt, 1367.6
 * W/m^2. Modern total solar irradiance measurements put it near 1361, and
 * PROJECT.md section 7 quotes that figure for exposure; the half a per cent
 * between them is far inside the uncertainty of Cr, and citing the file we
 * committed beats quoting a better number from memory. When the page is
 * refreshed the number follows it. */
static const EphBodyInfo BODIES[] = {
    { "sun",          6.957e8,   1367.6, NULL },
    { "mercury",      2.4394e6,  0.0,    NULL },
    { "venus",        6.05184e6, 0.0,    NULL },
    { "earth",        6.37101e6, 0.0,    &ATMOSPHERE_EARTH_USSA76 },
    { "moon",         1.73753e6, 0.0,    NULL },
    { "mars_bary",    3.38992e6, 0.0,    NULL },
    { "jupiter_bary", 6.9911e7,  0.0,    NULL },
    { "saturn_bary",  5.8232e7,  0.0,    NULL },
    { "uranus_bary",  2.5362e7,  0.0,    NULL },
    { "neptune_bary", 2.4624e7,  0.0,    NULL },
};
#define N_BODIES (sizeof BODIES / sizeof BODIES[0])

/* 120 days covers eight revolutions of the halo the scenarios fly, which is
 * enough to exercise multiple shooting over a span where the instability
 * matters, and keeps the file near 50 kB. */
#define SPAN_DAYS 120.0
#define INTERVAL_DAYS 8.0
#define DEGREE 14

/* Orientation needs two and a half times the coefficients position does over
 * the same interval, and the reason is in core/ephemeris.h: Earth's
 * quaternion is four full cycles of a wave across eight days, not a curve.
 * Measured on this very interval, the fit is off by 1.4 radians at degree
 * 24, by 8.8 m at the equator at 26, by a millimetre at 32, and by a
 * micrometre here - and the cooker prints the number, so lengthening
 * INTERVAL_DAYS without raising this cannot pass unnoticed. */
#define ORIENT_DEGREE 36

static const char *OUT_PATH = "data/fixture/earth_moon.eph";

static RefSample samples[MAX_SAMPLES];

int main(void)
{
    NBodySystem system;
    State initial[NBODY_MAX];
    RefGm gm_table[16];
    size_t n_gm = 0;
    char path[128];

    memset(&system, 0, sizeof system);
    system.n = N_BODIES;

    if (refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm)
        != CORE_OK) {
        fprintf(stderr, "cook: fixtures missing; run from the repository root\n");
        return 1;
    }

    for (size_t i = 0; i < N_BODIES; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", BODIES[i].name);

        size_t n = 0;
        CoreResult r = refdata_load_vectors(path, samples, MAX_SAMPLES, &n);
        if (r != CORE_OK && r != CORE_ERR_BUFFER_TOO_SMALL) {
            fprintf(stderr, "cook: cannot read %s\n", path);
            return 1;
        }
        if (n == 0) {
            fprintf(stderr, "cook: %s is empty\n", path);
            return 1;
        }

        initial[i] = samples[0].s;
        system.mu[i] = refdata_gm_of(gm_table, n_gm, BODIES[i].name);
        if (!(system.mu[i] > 0.0)) {
            fprintf(stderr, "cook: no GM for %s\n", BODIES[i].name);
            return 1;
        }

        /* Earth's oblateness (ROADMAP K2). Values are cited, not invented:
         * data/horizons/obj_earth.txt, "J2 (IERS 2010)" and "Equ. radius,
         * km". The Moon's own field is not here yet - GRAIL coefficients
         * are real data to import (K5), not a number to guess, and the
         * regression this is meant to shrink was already measured (ROADMAP
         * "Дві розвилки") to be about Earth's J2, not the Moon's.
         *
         * The pole is assumed fixed along the frame's z axis - see
         * nbody.c's comment on has_j2 for what that costs and why it is
         * acceptable before K3 gives bodies a real orientation. */
        if (strcmp(BODIES[i].name, "earth") == 0) {
            system.has_j2 = 1;
            system.j2_body = (int)i;
            system.j2_field.degree = 2;
            system.j2_field.re = 6378137.0;
            system.j2_field.c[harmonics_index(2, 0)] = -1.08262545e-3;
        }
    }

    EphBuildConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.t_begin = 0.0;
    cfg.t_end = SPAN_DAYS * DAY;
    cfg.interval_seconds = INTERVAL_DAYS * DAY;
    cfg.degree = DEGREE;
    cfg.orient_degree = ORIENT_DEGREE;

    /* Chosen from the fastest body, not from the size of the system - the
     * lesson of ROADMAP B5 - and tight enough to bind on its own, which is
     * the lesson of ex_ephspan: at the metre this was until now, the step
     * size came from the forced landings on fit nodes rather than from here,
     * and the cook was accurate by accident. Same number as the reversibility
     * diagnostic in ex_accuracy.c uses, and the cost of it is 0.04 s on a
     * ten-year cook. See EphBuildConfig::tol_m. */
    cfg.tol_m = 1.0e-6;

    EphBuildReport report;
    memset(&report, 0, sizeof report);

    if (eph_build(&system, initial, BODIES, &cfg, OUT_PATH, &report)
        != CORE_OK) {
        fprintf(stderr, "cook: build failed\n");
        return 1;
    }

    printf("%s\n", OUT_PATH);
    printf("  bodies    %zu\n", N_BODIES);
    printf("  span      %.0f days in %zu intervals\n", SPAN_DAYS,
           report.intervals);
    printf("  degree    %d\n", DEGREE);
    printf("  size      %zu bytes\n", report.bytes_written);
    printf("  steps     %ld\n", report.integrator_steps);
    printf("  fit error %.4g m\n", report.max_fit_error_m);
    printf("  orient    degree %d, error %.4g rad (%.4g m at Earth's equator)\n",
           ORIENT_DEGREE, report.max_orient_error_rad,
           report.max_orient_error_rad * 6378137.0);

    return 0;
}
