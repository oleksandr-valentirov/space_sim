/* Export: the solar system against JPL Horizons, over ten years.
 *
 *   horizons.csv   the divergence of the model from the published ephemeris,
 *                  sample by sample, for the full ten-body system and for the
 *                  three-body one beside it
 *
 * This is the regression that says the physics is the right physics rather
 * than merely self-consistent, and core/test/test_nbody.c states its purpose
 * exactly: the comparison is built to catch mistakes, not to reach JPL's
 * accuracy. A wrong frame, centre or unit shows up as hundreds of thousands of
 * kilometres. Missing physics shows up as far less.
 *
 * Which is why both systems are exported and not just the good one. The
 * three-body run is not a worse version of the ten-body run - it is the
 * control. Its error is the size of the physics that was left out, and having
 * it on the same axes is what makes the ten-body curve mean something. The
 * error growth also has a shape: a distortion of the orbits grows steadily,
 * while a frame or unit error is wrong at the first sample and stays wrong.
 *
 * Three error measures per sample, because they fail differently:
 *
 *   earth_m       Earth against Horizons in the barycentric frame, which
 *                 includes any bulk drift of the modelled subsystem
 *   earth_rel_m   the same with the subsystem's own barycentre removed, so
 *                 what remains is genuine distortion of the orbits
 *   moon_geo_m    the Moon relative to the Earth, which is the geometry the
 *                 game is actually built on
 *
 * Offline code: this is the mutual N-body integration the cooker runs, never
 * the runtime (core/offline/nbody.h). Run from the repository root. */

#include "csv.h"
#include "nbody.h"
#include "refdata.h"

#include <math.h>
#include <string.h>

#define MAX_SAMPLES 256

/* Every major body, in the order the fixtures use. */
static const char *ALL_BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

/* The system Milestone 0 started with, kept as the control described above. */
static const char *MINIMAL_BODIES[] = { "sun", "earth", "moon" };
#define N_MINIMAL (sizeof MINIMAL_BODIES / sizeof MINIMAL_BODIES[0])

#define TOLERANCE_M 1.0

static RefSample reference[N_ALL][MAX_SAMPLES];
static size_t n_samples;
static RefGm gm_table[16];
static size_t n_gm;

static int index_of(const char *name)
{
    for (size_t i = 0; i < N_ALL; i++) {
        if (strcmp(ALL_BODIES[i], name) == 0) {
            return (int)i;
        }
    }
    return -1;
}

static int load_fixtures(void)
{
    char path[128];

    for (size_t i = 0; i < N_ALL; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            fprintf(stderr, "ex_horizons: cannot load %s\n", path);
            return 0;
        }
        if (i == 0) {
            n_samples = n;
        } else if (n != n_samples) {
            fprintf(stderr, "ex_horizons: %s has %zu samples, expected %zu\n",
                    path, n, n_samples);
            return 0;
        }
    }

    return refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm)
           == CORE_OK;
}

static int run_model(Csv *c, const char *label, const char **names, size_t n)
{
    NBodySystem sys;
    memset(&sys, 0, sizeof sys);
    sys.n = n;

    int map[NBODY_MAX];
    int earth = -1, moon = -1;
    State current[NBODY_MAX];

    for (size_t i = 0; i < n; i++) {
        map[i] = index_of(names[i]);
        if (map[i] < 0) {
            return 0;
        }

        sys.mu[i] = refdata_gm_of(gm_table, n_gm, names[i]);
        if (sys.mu[i] <= 0.0) {
            fprintf(stderr, "ex_horizons: no GM for %s\n", names[i]);
            return 0;
        }

        current[i] = reference[map[i]][0].s;

        if (strcmp(names[i], "earth") == 0) {
            earth = (int)i;
        }
        if (strcmp(names[i], "moon") == 0) {
            moon = (int)i;
        }
    }

    if (earth < 0 || moon < 0) {
        return 0;
    }

    double energy0 = nbody_energy(&sys, current);
    Vec3d barycentre0 = nbody_barycentre(&sys, current);

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = TOLERANCE_M;
    cfg.max_steps = 5000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    /* Sample 0 is the initial condition itself, so every error is exactly
     * zero there. Written out all the same: a curve that starts at zero is
     * how a reader tells this is a propagation from the reference and not a
     * fit to it. */
    csv_named(c, label, 6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    for (size_t s = 1; s < n_samples; s++) {
        State next[NBODY_MAX];
        if (nbody_integrate(&sys, current, reference[0][s].s.t, &cfg, &st,
                            next) != CORE_OK) {
            fprintf(stderr, "ex_horizons: %s stopped at sample %zu\n",
                    label, s);
            return 0;
        }
        memcpy(current, next, sizeof next);

        State ref_now[NBODY_MAX];
        for (size_t i = 0; i < n; i++) {
            ref_now[i] = reference[map[i]][s].s;
        }

        Vec3d bary_model = nbody_barycentre(&sys, current);
        Vec3d bary_ref = nbody_barycentre(&sys, ref_now);

        double earth_m = vec3_distance(current[earth].r, ref_now[earth].r);
        double earth_rel_m = vec3_distance(
            vec3_sub(current[earth].r, bary_model),
            vec3_sub(ref_now[earth].r, bary_ref));
        double moon_geo_m = vec3_distance(
            vec3_sub(current[moon].r, current[earth].r),
            vec3_sub(ref_now[moon].r, ref_now[earth].r));

        double energy = nbody_energy(&sys, current);

        csv_named(c, label, 6,
                  (reference[0][s].s.t - reference[0][0].s.t) / 86400.0,
                  earth_m, earth_rel_m, moon_geo_m,
                  fabs((energy - energy0) / energy0),
                  vec3_distance(bary_model, barycentre0));
    }

    printf("  %-10s %zu bodies, %ld steps\n", label, n, st.n_accepted);
    return 1;
}

int main(void)
{
    if (!load_fixtures()) {
        fprintf(stderr, "  run from the repository root\n");
        return 1;
    }

    printf("ex_horizons: %zu reference epochs, %.1f years\n", n_samples,
           (reference[0][n_samples - 1].s.t - reference[0][0].s.t)
           / (365.25 * 86400.0));

    Csv c;
    if (!csv_open(&c, "build/csv/horizons.csv",
                  "system,days,earth_m,earth_rel_m,moon_geo_m,"
                  "energy_drift,barycentre_drift_m")) {
        return 1;
    }

    if (!run_model(&c, "ten_body", ALL_BODIES, N_ALL)
        || !run_model(&c, "three_body", MINIMAL_BODIES, N_MINIMAL)) {
        return 1;
    }

    return csv_close(&c) ? 0 : 1;
}
