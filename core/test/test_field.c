/* A vessel in the ephemeris field (ROADMAP C4).
 *
 * The oracle here is worth explaining, because a force model summing point
 * masses is the kind of code that is easy to write and hard to check: it
 * agrees with any independent implementation of the same formula, including a
 * wrong one.
 *
 * So the test does not reimplement the formula. It uses the fact that the
 * cooker integrated each body under exactly this acceleration - the sum over
 * every other body - and the answer is baked into the asset. Put a massless
 * particle on a body's own state, give it the field of all the OTHER bodies,
 * and it must follow that body. If any term, sign, index or unit is wrong, it
 * will not.
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "eph_build.h"
#include "field.h"
#include "refdata.h"
#include "stm.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define MAX_SAMPLES 256
#define DAY 86400.0

static const char *ALL_BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

#define EARTH 3
#define MOON  4

#define SPAN_DAYS 200.0

static const char *PATH = "build/test_field.eph";

static RefSample reference[N_ALL][MAX_SAMPLES];
static NBodySystem system_config;
static State initial[NBODY_MAX];

static int load_inputs(void)
{
    RefGm gm_table[16];
    size_t n_gm = 0;
    char path[128];

    memset(&system_config, 0, sizeof system_config);
    system_config.n = N_ALL;

    if (refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm)
        != CORE_OK) {
        return 0;
    }

    for (size_t i = 0; i < N_ALL; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            return 0;
        }
        system_config.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i]);
        initial[i] = reference[i][0].s;
        if (!(system_config.mu[i] > 0.0)) {
            return 0;
        }
    }

    return 1;
}

static Dop853Config vessel_config(void)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-3;
    cfg.max_steps = 20000000;
    return cfg;
}

/* Largest separation between a massless particle started on body's state and
 * the body itself, sampled through the span. */
static double tracking_error(const EphemerisCtx *eph, int body,
                             double t_begin, double t_end, int samples)
{
    FieldCtx field;
    if (field_all_but(eph, body, &field) != CORE_OK) {
        return -1.0;
    }

    State start;
    if (eph_body_state(eph, body, t_begin, &start) != CORE_OK) {
        return -1.0;
    }

    Dop853Config cfg = vessel_config();
    Dop853State st;
    memset(&st, 0, sizeof st);

    State vessel = start;
    double worst = 0.0;

    for (int k = 1; k <= samples; k++) {
        double t = t_begin + (t_end - t_begin) * (double)k / (double)samples;

        State next;
        if (dop853_integrate(accel_field, &field, &vessel, t, &cfg, &st, &next)
            != CORE_OK) {
            return -1.0;
        }
        vessel = next;

        State truth;
        if (eph_body_state(eph, body, t, &truth) != CORE_OK) {
            return -1.0;
        }

        double d = vec3_distance(vessel.r, truth.r);
        if (d > worst) {
            worst = d;
        }
    }

    if (field.failed) {
        return -1.0;
    }
    return worst;
}

int main(void)
{
    if (!load_inputs()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    EphBuildConfig build;
    memset(&build, 0, sizeof build);
    build.t_begin = 0.0;
    build.t_end = SPAN_DAYS * DAY;
    build.interval_seconds = 8.0 * DAY;
    build.degree = 14;
    build.tol_m = 1.0;

    EphBuildReport report;
    memset(&report, 0, sizeof report);
    CHECK(eph_build(&system_config, initial, ALL_BODIES, &build, PATH, &report)
          == CORE_OK);

    EphemerisCtx *eph = NULL;
    CHECK(eph_load(PATH, &eph) == CORE_OK);
    if (eph == NULL) {
        return EXIT_FAILURE;
    }

    double t_begin, t_end;
    CHECK(eph_span(eph, &t_begin, &t_end) == CORE_OK);

    /* Body selection. */
    {
        FieldCtx all;
        CHECK(field_all_bodies(eph, &all) == CORE_OK);
        CHECK(all.n_bodies == (int)N_ALL);
        CHECK(all.failed == 0);

        FieldCtx without;
        CHECK(field_all_but(eph, EARTH, &without) == CORE_OK);
        CHECK(without.n_bodies == (int)N_ALL - 1);
        for (int i = 0; i < without.n_bodies; i++) {
            CHECK(without.body[i] != EARTH);
        }

        CHECK(field_all_bodies(NULL, &all) == CORE_ERR_INVALID_ARG);
        CHECK(field_all_bodies(eph, NULL) == CORE_ERR_INVALID_ARG);
    }

    /* The oracle. A particle on the Earth's state, in the field of everything
     * but the Earth, follows the Earth; the same for the Moon, which is the
     * harder case because it is the fastest body in the set and the one whose
     * neighbourhood is most strongly curved.
     *
     * The residual is not zero and should not be: the particle feels the
     * field of the FITTED bodies, while the asset's trajectories came from the
     * exact integration that was then fitted. So this measures the Chebyshev
     * fit error, 4.6e-2 m, propagated through 200 days.
     *
     * Measured worst separation: 8.2 m for the Earth and 0.24 m for the Moon.
     * The Earth's grows steadily - 0.41, 1.2, 2.6, 4.4, 8.2 m at forty-day
     * intervals - because a small error in a nearly Keplerian orbit turns
     * into an along-track drift that accumulates. The Moon's oscillates
     * between 0.01 and 0.24 m and does not accumulate. Both are eight orders
     * of magnitude below what any real mistake would produce: a missing body,
     * a sign, or kilometres read as metres moves a planet by a fraction of
     * its own orbit. */
    {
        double earth = tracking_error(eph, EARTH, t_begin, t_end, 50);
        CHECK(earth >= 0.0);
        CHECK(earth < 100.0);

        double moon = tracking_error(eph, MOON, t_begin, t_end, 50);
        CHECK(moon >= 0.0);
        CHECK(moon < 100.0);

        CHECK(earth > 0.0);
        CHECK(moon > 0.0);
    }

    /* The gradient on its own, by central differences, at a point with no
     * symmetry to hide a transposed index. Measured agreement better than
     * 1e-6 relative. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* A million kilometres off the Earth, out of any plane. */
        Vec3d r = vec3(earth.r.x + 7.0e8, earth.r.y - 5.0e8, earth.r.z + 3.0e8);

        double g[9];
        field_gradient(t_begin, r, &field, g);

        const double eps = 1.0e3;

        for (int j = 0; j < 3; j++) {
            Vec3d rp = r, rm = r;
            double *pp = j == 0 ? &rp.x : (j == 1 ? &rp.y : &rp.z);
            double *pm = j == 0 ? &rm.x : (j == 1 ? &rm.y : &rm.z);
            *pp += eps;
            *pm -= eps;

            Vec3d ap, am;
            accel_field(t_begin, rp, vec3_zero(), &field, &ap);
            accel_field(t_begin, rm, vec3_zero(), &field, &am);

            double numeric[3] = {
                (ap.x - am.x) / (2.0 * eps),
                (ap.y - am.y) / (2.0 * eps),
                (ap.z - am.z) / (2.0 * eps),
            };

            for (int i = 0; i < 3; i++) {
                double scale = fabs(g[i * 3 + j]);
                CHECK(fabs(numeric[i] - g[i * 3 + j]) < 1e-5 * scale);
            }
        }

        /* Symmetric to the last bit, which is a property of how it is built
         * rather than of the arithmetic - see field_gradient. And traceless:
         * Laplace's equation holds for a sum of point-mass potentials
         * anywhere but at a source. The trace check costs nothing and fails
         * on a wrong coefficient in the outer product. */
        CHECK_BITS_EQ(g[1], g[3]);
        CHECK_BITS_EQ(g[2], g[6]);
        CHECK_BITS_EQ(g[5], g[7]);

        double trace = g[0] + g[4] + g[8];
        double scale = fabs(g[0]) + fabs(g[4]) + fabs(g[8]);
        CHECK(fabs(trace) < 1e-12 * scale);
    }

    /* The STM of a vessel trajectory, by finite differences. Same method as
     * core/test/test_stm.c, in metres this time: the perturbation is 1 km and
     * the integrator tolerance 1e-3 m, so the noise floor of the measurement
     * is around 1e-6 relative. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* A vessel well clear of the Earth, moving with it. */
        State vessel = earth;
        vessel.r.z += 1.0e9;
        vessel.t = t_begin;

        double t_stop = t_begin + 20.0 * DAY;

        Dop853Config cfg = vessel_config();
        Dop853State st;
        memset(&st, 0, sizeof st);

        double phi[STM_SIZE];
        State end;
        CHECK(stm_integrate(accel_field_var, &field, &vessel, t_stop, &cfg, &st,
                            &end, phi) == CORE_OK);
        CHECK(field.failed == 0);

        const double eps_r = 1.0e3;
        const double eps_v = 1.0e-3;

        double worst = 0.0;
        double biggest = 0.0;

        for (int j = 0; j < 6; j++) {
            double eps = j < 3 ? eps_r : eps_v;

            State plus = vessel, minus = vessel;
            double *pp[6] = { &plus.r.x, &plus.r.y, &plus.r.z,
                              &plus.v.x, &plus.v.y, &plus.v.z };
            double *pm[6] = { &minus.r.x, &minus.r.y, &minus.r.z,
                              &minus.v.x, &minus.v.y, &minus.v.z };
            *pp[j] += eps;
            *pm[j] -= eps;

            State end_plus, end_minus;
            memset(&st, 0, sizeof st);
            CHECK(dop853_integrate(accel_field, &field, &plus, t_stop, &cfg,
                                   &st, &end_plus) == CORE_OK);
            memset(&st, 0, sizeof st);
            CHECK(dop853_integrate(accel_field, &field, &minus, t_stop, &cfg,
                                   &st, &end_minus) == CORE_OK);

            double p[6] = { end_plus.r.x, end_plus.r.y, end_plus.r.z,
                            end_plus.v.x, end_plus.v.y, end_plus.v.z };
            double m[6] = { end_minus.r.x, end_minus.r.y, end_minus.r.z,
                            end_minus.v.x, end_minus.v.y, end_minus.v.z };

            for (int i = 0; i < 6; i++) {
                double numeric = (p[i] - m[i]) / (2.0 * eps);
                double d = fabs(numeric - phi[i * 6 + j]);
                if (d > worst) {
                    worst = d;
                }
                if (fabs(numeric) > biggest) {
                    biggest = fabs(numeric);
                }
            }
        }

        CHECK(biggest > 1.0);
        CHECK(worst < 1e-4 * biggest);
    }

    /* Running off the end of the ephemeris sets the flag rather than
     * returning a plausible zero. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);

        Vec3d a;
        accel_field(t_end + DAY, vec3(1.0e9, 0.0, 0.0), vec3_zero(), &field,
                    &a);
        CHECK(field.failed == 1);
        CHECK(vec3_norm(a) == 0.0);

        /* Sticky: a later good evaluation does not clear it. */
        accel_field(t_begin, vec3(1.0e9, 0.0, 0.0), vec3_zero(), &field, &a);
        CHECK(field.failed == 1);
        CHECK(vec3_norm(a) > 0.0);
    }

    eph_free(eph);
    remove(PATH);

    return TEST_RESULT();
}
