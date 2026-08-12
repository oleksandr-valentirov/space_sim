/* Mutual N-body against JPL Horizons (ROADMAP B5).
 *
 * The purpose of this comparison is to catch mistakes, not to reach JPL's
 * accuracy. A wrong frame, centre or unit shows up as hundreds of thousands
 * of kilometres or more; missing physics shows up as far less. The test is
 * built to tell those two apart, because only the first is a bug.
 *
 * Run from the repository root. */

#include "nbody.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_SAMPLES 256
#define AU_M 1.495978707e11

/* Order matters: the first three are the Milestone 0 system, and the rest are
 * appended one at a time to see what each contributes. */
static const char *BODY_NAMES[] = {
    "sun", "earth", "moon", "venus", "mars_bary", "jupiter_bary",
};
#define N_BODIES (sizeof BODY_NAMES / sizeof BODY_NAMES[0])

static RefSample reference[N_BODIES][MAX_SAMPLES];
static size_t n_samples;
static RefGm gm_table[16];
static size_t n_gm;

typedef struct {
    double max_earth;            /* against Horizons, in the SSB frame */
    double max_earth_relative;   /* with the model's own barycentre removed */
    double max_moon;
    double max_moon_geocentric;  /* Moon relative to Earth: the geometry the
                                  * game actually cares about */
    double energy_drift;
    double barycentre_drift;
    long   steps;
} RunResult;

static int load_fixtures(void)
{
    char path[128];

    for (size_t i = 0; i < N_BODIES; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", BODY_NAMES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n) != CORE_OK) {
            fprintf(stderr, "  cannot load %s\n", path);
            return 0;
        }
        if (i == 0) {
            n_samples = n;
        } else if (n != n_samples) {
            fprintf(stderr, "  %s has %zu samples, expected %zu\n",
                    path, n, n_samples);
            return 0;
        }
    }

    return refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm) == CORE_OK;
}

/* Integrates the first n bodies from J2000 across the whole fixture, stopping
 * at every reference epoch to measure the divergence. */
static RunResult run_model(size_t n, double tol_m)
{
    RunResult out;
    memset(&out, 0, sizeof out);

    NBodySystem sys;
    memset(&sys, 0, sizeof sys);
    sys.n = n;

    State current[NBODY_MAX];
    for (size_t i = 0; i < n; i++) {
        sys.mu[i] = refdata_gm_of(gm_table, n_gm, BODY_NAMES[i]);
        CHECK(sys.mu[i] > 0.0);
        current[i] = reference[i][0].s;
    }

    double energy0 = nbody_energy(&sys, current);
    Vec3d barycentre0 = nbody_barycentre(&sys, current);

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol_m;
    cfg.max_steps = 5000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    for (size_t s = 1; s < n_samples; s++) {
        State next[NBODY_MAX];
        CoreResult r = nbody_integrate(&sys, current, reference[0][s].s.t,
                                       &cfg, &st, next);
        CHECK(r == CORE_OK);
        if (r != CORE_OK) {
            return out;
        }
        memcpy(current, next, sizeof next);

        /* The same barycentre, computed from the model and from the
         * reference with the same bodies and the same weights. Subtracting it
         * removes the bulk translation of the subsystem and leaves the part
         * of the error that is actually a distortion of the orbits. */
        State ref_now[NBODY_MAX];
        for (size_t i = 0; i < n; i++) {
            ref_now[i] = reference[i][s].s;
        }
        Vec3d bary_model = nbody_barycentre(&sys, current);
        Vec3d bary_ref = nbody_barycentre(&sys, ref_now);

        double d_earth = vec3_distance(current[1].r, reference[1][s].s.r);
        double d_moon = vec3_distance(current[2].r, reference[2][s].s.r);

        double d_earth_rel = vec3_distance(vec3_sub(current[1].r, bary_model),
                                           vec3_sub(reference[1][s].s.r, bary_ref));
        double d_moon_geo = vec3_distance(vec3_sub(current[2].r, current[1].r),
                                          vec3_sub(reference[2][s].s.r,
                                                   reference[1][s].s.r));

        if (d_earth > out.max_earth) {
            out.max_earth = d_earth;
        }
        if (d_moon > out.max_moon) {
            out.max_moon = d_moon;
        }
        if (d_earth_rel > out.max_earth_relative) {
            out.max_earth_relative = d_earth_rel;
        }
        if (d_moon_geo > out.max_moon_geocentric) {
            out.max_moon_geocentric = d_moon_geo;
        }
    }

    double energy1 = nbody_energy(&sys, current);
    out.energy_drift = fabs((energy1 - energy0) / energy0);
    out.barycentre_drift = vec3_distance(nbody_barycentre(&sys, current),
                                         barycentre0);
    out.steps = st.n_accepted;
    return out;
}

int main(void)
{
    if (!load_fixtures()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }
    CHECK(n_samples == 122);

    /* The N-body integrator against the two-body one. Give the second body
     * zero mass and it becomes a test particle in the first body's field,
     * which is exactly the problem dop853_integrate solves. Two
     * implementations agreeing is the check; the shared coefficient table is
     * the only thing they have in common. */
    {
        NBodySystem sys;
        memset(&sys, 0, sizeof sys);
        sys.n = 2;
        sys.mu[0] = 3.98600435436e14;
        sys.mu[1] = 0.0;

        double radius = 7.0e6;
        double speed = sqrt(sys.mu[0] / radius);

        State y[2];
        y[0] = (State){ { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, 0.0 };
        y[1] = (State){ { radius, 0.0, 0.0 }, { 0.0, speed, 0.0 }, 0.0 };

        double period = two_body_period(y[1].r, y[1].v, sys.mu[0]);

        Dop853Config cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-6;

        Dop853State st_n;
        memset(&st_n, 0, sizeof st_n);
        State out_n[2];
        CHECK(nbody_integrate(&sys, y, 5.0 * period, &cfg, &st_n, out_n)
              == CORE_OK);

        TwoBodyCtx ctx = { sys.mu[0] };
        Dop853State st_1;
        memset(&st_1, 0, sizeof st_1);
        State out_1;
        CHECK(dop853_integrate(accel_two_body, &ctx, &y[1], 5.0 * period,
                               &cfg, &st_1, &out_1) == CORE_OK);

        CHECK(vec3_distance(out_n[1].r, out_1.r) < 1e-3);

        /* A massless body pulls on nothing, so the attractor must not move. */
        CHECK(vec3_norm(out_n[0].r) < 1e-9);
    }

    /* Baseline: Sun, Earth and Moon as point masses, ten years.
     *
     * Measured maxima against Horizons: 5.415e9 m for the Earth, 5.544e9 m
     * for the Moon. Large, and correctly so - this model is missing every
     * other planet. What matters is the order of magnitude: a wrong frame or
     * centre would put this above 1e11 m, an AU or more. */
    RunResult baseline = run_model(3, 1e1);
    {
        CHECK(baseline.max_earth > 1e8);
        CHECK(baseline.max_earth < 5e10);
        CHECK(baseline.max_moon < 5e10);

        /* Energy of the isolated three-body system is conserved by the
         * dynamics, so its drift measures the integrator rather than the
         * model. Separating those two is the whole point of this test.
         *
         * Measured at tol 1e1 m: 4.23e-11, and it tracks the tolerance
         * closely - 9.10e-9 at tol 1e3, 2.99e-12 at tol 1e0. */
        CHECK(baseline.energy_drift < 1e-9);

        /* Nearly all of the error is the subsystem translating as a whole,
         * not its orbits deforming. Measured 5.417e9 m in the SSB frame
         * against 5.605e7 m once the model's own barycentre is removed - a
         * factor of 97.
         *
         * The reason is momentum: three bodies carrying only part of the
         * solar system's momentum have a barycentre that drifts in a
         * straight line through the SSB frame. Measured drift 4.959e9 m,
         * identical at every tolerance from 1e5 down to 1e0, which is what
         * proves it is physics and not integration error. */
        CHECK(baseline.max_earth_relative < baseline.max_earth / 20.0);

        /* And the geometry the game is actually built on - where the Moon is
         * relative to the Earth - is far better still: 4.98e5 m over ten
         * years from three point masses. Half a megametre on a 384000 km
         * orbit. */
        CHECK(baseline.max_moon_geocentric < 2e6);
    }

    /* Jupiter is the dominant missing perturber. Adding it must help, and
     * measurably: 5.415e9 -> 8.331e8 m for the Earth, a factor of 6.5.
     *
     * Venus and Mars, measured at 5.428e9 and 5.421e9, change nothing at this
     * error level. That is not a failure of the model - they are simply far
     * below the residual, which is dominated by the planets still missing.
     * The ROADMAP expectation that every added body reduces the error was too
     * strong; only the dominant one does. */
    RunResult with_jupiter = run_model(6, 1e1);
    {
        CHECK(with_jupiter.max_earth < baseline.max_earth / 3.0);
        CHECK(with_jupiter.max_moon < baseline.max_moon / 3.0);
        CHECK(with_jupiter.energy_drift < 1e-9);

        /* Same story about the frame: 8.357e8 m absolute against 1.358e7 m
         * relative, a factor of 62, with a barycentre drift of 8.886e8 m that
         * is again tolerance-independent. */
        CHECK(with_jupiter.max_earth_relative < with_jupiter.max_earth / 20.0);
        CHECK(with_jupiter.barycentre_drift < baseline.barycentre_drift);

        /* Worth recording because it is the one number that got worse:
         * geocentric lunar error 9.07e5 m against the three-body model's
         * 4.98e5 m. Adding planets moves the Earth's orbit, which shifts the
         * phase of the Sun's perturbation on the Moon; the three-body value
         * benefits from a cancellation that has no particular right to hold.
         * Not a regression to chase - a reminder that a single number is not
         * a measure of a model. */
        CHECK(with_jupiter.max_moon_geocentric < 3e6);
    }

    /* The residual is model error, not integration error - which is the
     * single most useful thing this test establishes. Tightening the
     * tolerance by four orders of magnitude moves the answer by under a
     * percent: measured 8.341e8 m at tol 1e5 and 8.357e8 m at tol 1e1.
     *
     * So there is no point tuning the integrator to close this gap. The gap
     * closes by adding bodies. */
    {
        RunResult loose = run_model(6, 1e5);
        RunResult tight = run_model(6, 1e1);

        CHECK(tight.steps > loose.steps * 2);

        double relative_change = fabs(tight.max_earth - loose.max_earth)
                               / loose.max_earth;
        CHECK(relative_change < 0.05);
    }

    /* Sanity of the fixtures as used here, independent of the integration:
     * the Earth starts about an AU from the barycentre and the Moon starts
     * near the Earth. If either were false, everything above would be
     * measuring the wrong thing. */
    {
        CHECK(fabs(vec3_norm(reference[1][0].s.r) / AU_M - 1.0) < 0.05);
        CHECK(vec3_distance(reference[2][0].s.r, reference[1][0].s.r) < 4.1e8);
    }

    return TEST_RESULT();
}
