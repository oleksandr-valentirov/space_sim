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
static const char *BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_BODIES (sizeof BODIES / sizeof BODIES[0])

/* 120 days covers eight revolutions of the halo the scenarios fly, which is
 * enough to exercise multiple shooting over a span where the instability
 * matters, and keeps the file near 50 kB. */
#define SPAN_DAYS 120.0
#define INTERVAL_DAYS 8.0
#define DEGREE 14

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
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", BODIES[i]);

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
        system.mu[i] = refdata_gm_of(gm_table, n_gm, BODIES[i]);
        if (!(system.mu[i] > 0.0)) {
            fprintf(stderr, "cook: no GM for %s\n", BODIES[i]);
            return 1;
        }
    }

    EphBuildConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.t_begin = 0.0;
    cfg.t_end = SPAN_DAYS * DAY;
    cfg.interval_seconds = INTERVAL_DAYS * DAY;
    cfg.degree = DEGREE;

    /* Chosen from the fastest body, not from the size of the system - the
     * lesson of ROADMAP B5. */
    cfg.tol_m = 1.0;

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

    return 0;
}
