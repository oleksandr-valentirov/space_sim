/* The ephemeris asset: cook it, read it back, break it (ROADMAP B6).
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "eph_build.h"
#include "ephemeris.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>
#include <stdint.h>

#define MAX_SAMPLES 256
#define DAY 86400.0

static const char *ALL_BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

#define SPAN_DAYS 60.0
#define INTERVAL_DAYS 8.0
#define DEGREE 14

static const char *PATH_A = "build/test_eph_a.eph";
static const char *PATH_B = "build/test_eph_b.eph";

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

    if (refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm) != CORE_OK) {
        return 0;
    }

    for (size_t i = 0; i < N_ALL; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n) != CORE_OK) {
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

static EphBuildConfig default_config(void)
{
    EphBuildConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.t_begin = 0.0;
    cfg.t_end = SPAN_DAYS * DAY;
    cfg.interval_seconds = INTERVAL_DAYS * DAY;
    cfg.degree = DEGREE;
    cfg.tol_m = 1.0;
    return cfg;
}

/* Reads a whole file so two builds can be compared byte for byte. */
static size_t slurp(const char *path, unsigned char *buf, size_t cap)
{
    FILE *f = fopen(path, "rb");
    if (f == NULL) {
        return 0;
    }
    size_t n = fread(buf, 1, cap, f);
    fclose(f);
    return n;
}

static void write_corrupt(const char *src, const char *dst,
                          size_t offset, const void *bytes, size_t n)
{
    static unsigned char buf[1 << 20];
    size_t size = slurp(src, buf, sizeof buf);
    CHECK(size > offset + n);

    memcpy(buf + offset, bytes, n);

    FILE *f = fopen(dst, "wb");
    CHECK(f != NULL);
    if (f != NULL) {
        fwrite(buf, 1, size, f);
        fclose(f);
    }
}

int main(void)
{
    if (!load_inputs()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    EphBuildConfig cfg = default_config();
    EphBuildReport report;
    memset(&report, 0, sizeof report);

    CHECK(eph_build(&system_config, initial, ALL_BODIES, &cfg, PATH_A, &report)
          == CORE_OK);

    /* Measured for this configuration: 8 intervals, 27328 bytes, 189
     * integrator steps, and a fit error of 4.63e-2 m at the least constrained
     * point of each interval.
     *
     * The degree and interval come from a sweep over one year of the full
     * ten-body system:
     *
     *   interval  degree   fit error      size per year
     *   8 d       10       4.26e+1 m      108 KB
     *   8 d       14       5.21e-2 m      151 KB
     *   8 d       18       1.32e-2 m      195 KB
     *   4 d       14       4.77e-3 m      302 KB
     *   2 d       10       5.46e-3 m      429 KB
     *
     * Degree 18 is worse than 14 at every interval below 8 days, which is the
     * fit reaching its rounding floor rather than anything physical. 8 days
     * with degree 14 buys 5 cm for 151 KB a year - about 30 MB for the 200
     * years PROJECT.md asks the ephemeris to cover. */
    {
        CHECK(report.intervals == 8);
        CHECK(report.max_fit_error_m < 1.0);
        CHECK(report.max_fit_error_m > 0.0);
        CHECK(report.integrator_steps > 0);

        /* Spelled out from core/ephemeris.h's format table rather than
         * recorded as a number, so it says what the layout IS and not
         * merely what it measured once. Every body here is a point mass -
         * this system sets no harmonics - so each carries the degree word
         * and nothing after it.
         *
         * The recorded-number version of this check was 27328 and went
         * stale the moment version 2 added that word (ROADMAP K4b), which
         * is exactly the right failure: a format change that did not move
         * the size would slip past a hardcoded total. */
        size_t header = 8 + 4 + 4 + 4 + 4 + 8 + 8 + 8;
        size_t per_body = EPH_NAME_SIZE + sizeof(double) + sizeof(uint32_t);
        size_t coeffs = (size_t)report.intervals * N_ALL * 3u * 14u
                      * sizeof(double);
        CHECK(report.bytes_written == header + N_ALL * per_body + coeffs);
    }

    EphemerisCtx *eph = NULL;
    CHECK(eph_load(PATH_A, &eph) == CORE_OK);
    if (eph == NULL) {
        return TEST_RESULT();
    }

    /* Metadata survives the round trip. */
    {
        CHECK(eph_body_count(eph) == (int)N_ALL);

        for (size_t i = 0; i < N_ALL; i++) {
            const char *name = eph_body_name(eph, (int)i);
            CHECK(name != NULL && strcmp(name, ALL_BODIES[i]) == 0);
            CHECK_BITS_EQ(eph_body_mu(eph, (int)i), system_config.mu[i]);
        }

        CHECK(eph_body_name(eph, -1) == NULL);
        CHECK(eph_body_name(eph, (int)N_ALL) == NULL);
        CHECK_BITS_EQ(eph_body_mu(eph, (int)N_ALL), 0.0);

        double t0 = 0.0, t1 = 0.0;
        CHECK(eph_span(eph, &t0, &t1) == CORE_OK);
        CHECK_BITS_EQ(t0, 0.0);
        CHECK_BITS_EQ(t1, 8.0 * INTERVAL_DAYS * DAY);
    }

    double t_begin = 0.0, t_end = 0.0;
    eph_span(eph, &t_begin, &t_end);

    /* Against the integrator it was built from. This is looser than the
     * cooker's own fit error and correctly so: the comparison integrates
     * independently from the same start, so it accumulates its own path
     * error on top of the fit. Measured 4.38e1 m for position and 9.19e-5
     * m/s for velocity over sixty days - dominated by the two integrations
     * diverging, not by the polynomial.
     *
     * "The same start" is what makes this work, and it stopped being true the
     * moment the cooker began anchoring the barycentre: this run failed at
     * 1e3 m immediately, because it was integrating a system carrying the
     * residual momentum that the asset no longer has. The test was right and
     * the omission was here. Anchoring the copy below restores the
     * comparison; it does not paper over anything, because the quantity being
     * checked is agreement between the asset and its own integrator, not
     * agreement with any particular frame. */
    {
        Dop853Config integ;
        memset(&integ, 0, sizeof integ);
        integ.tol_m = 1.0;
        integ.max_steps = 5000000;

        Dop853State st;
        memset(&st, 0, sizeof st);

        State current[NBODY_MAX];
        memcpy(current, initial, sizeof current);
        if (eph_anchor_enabled()) {
            nbody_anchor_barycentre(&system_config, current);
        }

        double max_position = 0.0;
        double max_velocity = 0.0;

        for (int k = 1; k <= 30; k++) {
            double t = t_begin + (t_end - t_begin) * (double)k / 30.0;
            State next[NBODY_MAX];
            CHECK(nbody_integrate(&system_config, current, t, &integ, &st, next)
                  == CORE_OK);
            memcpy(current, next, sizeof next);

            for (size_t i = 0; i < N_ALL; i++) {
                State from_asset;
                CHECK(eph_body_state(eph, (int)i, t, &from_asset) == CORE_OK);

                double dp = vec3_distance(from_asset.r, current[i].r);
                double dv = vec3_norm(vec3_sub(from_asset.v, current[i].v));
                if (dp > max_position) {
                    max_position = dp;
                }
                if (dv > max_velocity) {
                    max_velocity = dv;
                }
                CHECK_BITS_EQ(from_asset.t, t);
            }
        }

        CHECK(max_position < 1e3);
        CHECK(max_velocity < 1e-2);
    }

    /* Velocity is the analytic derivative of the stored polynomial, so it
     * must agree with a finite difference of the position the same asset
     * reports - away from interval boundaries.
     *
     * Measured 3.11e-4 m/s, worst for Neptune, and that is the finite
     * difference's own rounding rather than an error in the derivative:
     * Neptune sits 4.5e12 m out, where one ULP of position is 5e-4 m, and
     * dividing that by a two second baseline gives exactly this. */
    {
        double worst = 0.0;
        double interval = INTERVAL_DAYS * DAY;

        for (size_t i = 0; i < N_ALL; i++) {
            for (int k = 0; k < 7; k++) {
                double a = t_begin + (double)k * interval + 0.2 * interval;
                double b = t_begin + (double)k * interval + 0.8 * interval;

                for (int j = 0; j <= 50; j++) {
                    double t = a + (b - a) * (double)j / 50.0;
                    State before, after, at;
                    CHECK(eph_body_state(eph, (int)i, t - 1.0, &before) == CORE_OK);
                    CHECK(eph_body_state(eph, (int)i, t + 1.0, &after) == CORE_OK);
                    CHECK(eph_body_state(eph, (int)i, t, &at) == CORE_OK);

                    Vec3d fd = vec3_scale(vec3_sub(after.r, before.r), 0.5);
                    double d = vec3_norm(vec3_sub(fd, at.v));
                    if (d > worst) {
                        worst = d;
                    }
                }
            }
        }

        CHECK(worst < 1e-2);
    }

    /* A piecewise fit is not continuous across its joins, and pretending
     * otherwise would be the sort of assumption that surfaces much later as
     * an unexplained jitter in a trajectory. Measured at the seven interior
     * boundaries: 1.17e-1 m in position and 2.33e-5 m/s in velocity, both of
     * the order of the fit error, which is what they should be. */
    {
        double worst_position = 0.0;
        double worst_velocity = 0.0;
        double interval = INTERVAL_DAYS * DAY;

        for (int k = 1; k < 8; k++) {
            double boundary = t_begin + (double)k * interval;

            for (size_t i = 0; i < N_ALL; i++) {
                State left, right;
                CHECK(eph_body_state(eph, (int)i, boundary - 1e-6, &left) == CORE_OK);
                CHECK(eph_body_state(eph, (int)i, boundary + 1e-6, &right) == CORE_OK);

                double dp = vec3_distance(left.r, right.r);
                double dv = vec3_norm(vec3_sub(left.v, right.v));
                if (dp > worst_position) {
                    worst_position = dp;
                }
                if (dv > worst_velocity) {
                    worst_velocity = dv;
                }
            }
        }

        CHECK(worst_position < 10.0);
        CHECK(worst_velocity < 1e-3);
    }

    /* Outside the span is an error, not an extrapolation. A Chebyshev fit
     * evaluated past its interval produces confident nonsense. */
    {
        State out;
        CHECK(eph_body_state(eph, 0, t_begin - 1.0, &out) == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_state(eph, 0, t_end + 1.0, &out) == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_state(eph, -1, t_begin, &out) == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_state(eph, (int)N_ALL, t_begin, &out) == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_state(eph, 0, t_begin, NULL) == CORE_ERR_INVALID_ARG);

        /* Both endpoints of the span are inside it. The last one lands
         * exactly where the interval index would run off the end. */
        CHECK(eph_body_state(eph, 0, t_begin, &out) == CORE_OK);
        CHECK(eph_body_state(eph, 0, t_end, &out) == CORE_OK);
    }

    eph_free(eph);
    eph_free(NULL);   /* must be a no-op, like free */

    /* The cooker is reproducible on this machine: same inputs, same bytes.
     * That is what makes the asset shippable - a player's copy is the
     * developer's copy, and the determinism argument in PROJECT.md section 4
     * rests on that rather than on everyone recomputing it. */
    {
        EphBuildReport second;
        memset(&second, 0, sizeof second);
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &cfg, PATH_B,
                        &second) == CORE_OK);

        static unsigned char a[1 << 20];
        static unsigned char b[1 << 20];
        size_t na = slurp(PATH_A, a, sizeof a);
        size_t nb = slurp(PATH_B, b, sizeof b);

        CHECK(na > 0 && na == nb);
        CHECK(memcmp(a, b, na) == 0);
        CHECK(second.bytes_written == report.bytes_written);
        CHECK_BITS_EQ(second.max_fit_error_m, report.max_fit_error_m);
    }

    /* Corrupted files are rejected rather than read as garbage. */
    {
        EphemerisCtx *bad = NULL;

        write_corrupt(PATH_A, PATH_B, 0, "XXXX", 4);           /* magic */
        CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);
        CHECK(bad == NULL);

        unsigned wrong_version = EPH_VERSION + 1u;
        write_corrupt(PATH_A, PATH_B, 8, &wrong_version, sizeof wrong_version);
        CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);

        /* The sentinel is the guard against a file written by a machine that
         * disagrees about byte order or floating point layout. */
        double wrong_sentinel = 2.0;
        write_corrupt(PATH_A, PATH_B, 40, &wrong_sentinel, sizeof wrong_sentinel);
        CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);

        /* Truncation: the header promises more coefficients than follow. */
        {
            static unsigned char buf[1 << 20];
            size_t size = slurp(PATH_A, buf, sizeof buf);
            FILE *f = fopen(PATH_B, "wb");
            CHECK(f != NULL);
            if (f != NULL) {
                fwrite(buf, 1, size - 64, f);
                fclose(f);
            }
            CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);
        }

        /* And trailing bytes mean the file is not what its header says. */
        {
            static unsigned char buf[1 << 20];
            size_t size = slurp(PATH_A, buf, sizeof buf);
            FILE *f = fopen(PATH_B, "wb");
            CHECK(f != NULL);
            if (f != NULL) {
                fwrite(buf, 1, size, f);
                fwrite("junk", 1, 4, f);
                fclose(f);
            }
            CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);
        }

        CHECK(eph_load("build/definitely_not_here.eph", &bad)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_load(NULL, &bad) == CORE_ERR_INVALID_ARG);
        CHECK(eph_load(PATH_A, NULL) == CORE_ERR_INVALID_ARG);
    }

    /* Cooker argument validation. */
    {
        EphBuildConfig bad = default_config();
        bad.degree = 1;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);

        bad = default_config();
        bad.t_end = bad.t_begin;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);

        bad = default_config();
        bad.interval_seconds = 0.0;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);

        bad = default_config();
        bad.tol_m = 0.0;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);
    }

    remove(PATH_A);
    remove(PATH_B);

    return TEST_RESULT();
}
