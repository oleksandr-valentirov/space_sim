/* The ephemeris asset: cook it, read it back, break it (ROADMAP B6).
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "body_rotation.h"
#include "cheb_fit.h"
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

static const EphBodyInfo ALL_BODIES[] = {
    { "sun", 0.0, 0.0, NULL, NULL },          { "mercury", 0.0, 0.0, NULL, NULL },
    { "venus", 0.0, 0.0, NULL, NULL },        { "earth", 0.0, 0.0, NULL, NULL },
    { "moon", 0.0, 0.0, NULL, NULL },         { "mars_bary", 0.0, 0.0, NULL, NULL },
    { "jupiter_bary", 0.0, 0.0, NULL, NULL }, { "saturn_bary", 0.0, 0.0, NULL, NULL },
    { "uranus_bary", 0.0, 0.0, NULL, NULL },  { "neptune_bary", 0.0, 0.0, NULL, NULL },
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

#define SPAN_DAYS 60.0
#define INTERVAL_DAYS 8.0
#define DEGREE 14

/* Two of the ten bodies above have a rotation model, so this asset carries
 * orientation for exactly two (ROADMAP K3b). Same degree as the shipped
 * fixture and for the same measured reason - core/ephemeris.h. */
#define ORIENT_DEGREE 36
#define N_ORIENT 2

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
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i].name);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n) != CORE_OK) {
            return 0;
        }
        system_config.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i].name);
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
    cfg.orient_degree = ORIENT_DEGREE;
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
         * the size would slip past a hardcoded total. Version 3 moved it
         * again by two doubles a body, and this check named the two. */
        size_t header = 8 + 4 + 4 + 4 + 4 + 4 + 8 + 8 + 8;
        size_t per_body = EPH_NAME_SIZE
                        + sizeof(double)      /* mu */
                        + sizeof(double)      /* mean radius (K6b) */
                        + sizeof(double)      /* solar flux (K6b) */
                        + sizeof(uint32_t)    /* orientation flag (K3b) */
                        + sizeof(uint32_t)    /* harmonic degree */
                        + sizeof(uint32_t);   /* atmosphere layers (K7b) */
        size_t coeffs = (size_t)report.intervals * N_ALL * 3u * DEGREE
                      * sizeof(double);
        size_t orient = (size_t)report.intervals * N_ORIENT * 4u * ORIENT_DEGREE
                      * sizeof(double);
        CHECK(report.bytes_written == header + N_ALL * per_body + coeffs
                                    + orient);

        /* Version 4's channels cost more than everything else in the file
         * put together - 18432 bytes of orientation against 26880 of
         * position, for two bodies out of ten - which is the price of
         * fitting a wave with a polynomial. Worth stating next to the
         * layout, since the obvious way to make it cheaper (fewer
         * coefficients) is exactly the one that does not work.
         *
         * Version 5 moved it again, by one word a body - every body in this
         * system is airless, so none of them carries a layer after it. */
        CHECK(orient > 0);
    }

    /* The orientation fit is measured, not assumed: the cooker reports the
     * worst it saw at the least constrained point of any interval. Measured
     * 1.97e-14 rad over these sixty days, which is 0.13 micrometres at
     * Earth's equator; the shipped fixture's hundred and twenty days report
     * 1.24e-13. The upper bound is what has teeth - degree 24 over this same
     * interval would report 1.4 rad, and catching that is what it is for. */
    {
        CHECK(report.max_orient_error_rad > 0.0);
        CHECK(report.max_orient_error_rad < 1e-10);
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
            CHECK(name != NULL && strcmp(name, ALL_BODIES[i].name) == 0);
            CHECK_BITS_EQ(eph_body_mu(eph, (int)i), system_config.mu[i]);

            /* ROADMAP K6b. This system leaves both at zero, which is the
             * value that means "the asset does not say" - so what is being
             * checked here is that the reader does not invent one. */
            CHECK_BITS_EQ(eph_body_radius(eph, (int)i), ALL_BODIES[i].radius_m);
            CHECK_BITS_EQ(eph_body_flux(eph, (int)i), ALL_BODIES[i].flux_1au);
        }

        CHECK(eph_body_name(eph, -1) == NULL);
        CHECK(eph_body_name(eph, (int)N_ALL) == NULL);
        CHECK_BITS_EQ(eph_body_radius(eph, -1), 0.0);
        CHECK_BITS_EQ(eph_body_flux(eph, (int)N_ALL), 0.0);
        CHECK_BITS_EQ(eph_body_mu(eph, (int)N_ALL), 0.0);

        double t0 = 0.0, t1 = 0.0;
        CHECK(eph_span(eph, &t0, &t1) == CORE_OK);
        CHECK_BITS_EQ(t0, 0.0);
        CHECK_BITS_EQ(t1, 8.0 * INTERVAL_DAYS * DAY);
    }

    double t_begin = 0.0, t_end = 0.0;
    eph_span(eph, &t_begin, &t_end);

    /* Orientation survives the round trip through the file (ROADMAP K3b).
     *
     * The oracle is body_rotation.c itself, which is not circular here: that
     * side computes a rotation with sin() and cos() at one instant, this side
     * evaluates four Chebyshev polynomials the cooker fitted through 36 such
     * instants and renormalises. Every part of the path between them - the
     * sign chain, the layout, the interval arithmetic, the slot a body's
     * channels sit in - is what can go wrong, and all of it would show up as
     * a mismatch here.
     *
     * Sampled away from the nodes and away from interval boundaries, since
     * the fit is exact at its nodes. Measured worst: 1.22e-13 per
     * component, sampled far more densely than the cooker's own one probe
     * per interval, which is why it is the larger of the two numbers. */
    {
        double worst = 0.0;

        for (size_t i = 0; i < N_ALL; i++) {
            for (int k = 0; k <= 120; k++) {
                double t = t_begin + (t_end - t_begin) * (double)k / 120.5;

                Quat from_asset;
                CHECK(eph_body_orientation(eph, (int)i, t, &from_asset)
                      == CORE_OK);

                if (!body_rotation_has_model(ALL_BODIES[i].name)) {
                    /* A body with no model is the identity exactly, not
                     * approximately: no channels were written for it, so
                     * there is nothing for a fit to round. */
                    CHECK_BITS_EQ(from_asset.w, 1.0);
                    CHECK_BITS_EQ(from_asset.x, 0.0);
                    CHECK_BITS_EQ(from_asset.y, 0.0);
                    CHECK_BITS_EQ(from_asset.z, 0.0);
                    continue;
                }

                Quat truth;
                CHECK(body_rotation_of(ALL_BODIES[i].name, t, &truth)
                      == CORE_OK);

                /* q and -q are the same rotation; the cooker's sign chain
                 * may have settled on either. */
                double dot = from_asset.w * truth.w + from_asset.x * truth.x
                           + from_asset.y * truth.y + from_asset.z * truth.z;
                double s = dot < 0.0 ? -1.0 : 1.0;

                double d[4] = { from_asset.w - s * truth.w,
                                from_asset.x - s * truth.x,
                                from_asset.y - s * truth.y,
                                from_asset.z - s * truth.z };
                for (int c = 0; c < 4; c++) {
                    if (fabs(d[c]) > worst) {
                        worst = fabs(d[c]);
                    }
                }

                /* Unit length is the one invariant four independent fits do
                 * not preserve on their own, and eph_body_orientation is
                 * where it is restored. */
                CHECK(fabs(quat_norm_sq(from_asset) - 1.0) < 1e-15);
            }
        }

        CHECK(worst < 1e-11);
    }

    /* Angular velocity (ROADMAP K7b), against a number that was in the
     * repository before this function existed: obj_earth.txt's
     * "Rot. Rate (rad/s) = 0.00007292115", and obj_moon.txt's sidereal rate.
     * The same external oracle K3a used for the orientation itself, which is
     * the point - this reads the derivative of the same fitted channels, so
     * checking it against those channels would be checking a polynomial
     * against itself.
     *
     * Direction matters as much as magnitude and is checked separately: for
     * both bodies the rotation is prograde about the pole, so omega must
     * point along the axis eph_body_orientation puts the body's z on, not
     * against it. A sign error here would leave the magnitude perfect and
     * make the atmosphere blow the wrong way at 930 m/s. */
    {
        static const struct {
            int    body;
            double rate;
        } SPIN[] = { { 3, 7.292115e-5 }, { 4, 2.6617e-6 } };

        for (size_t k = 0; k < sizeof SPIN / sizeof SPIN[0]; k++) {
            double t = t_begin + 11.0 * DAY;

            Vec3d w;
            CHECK(eph_body_angular_velocity(eph, SPIN[k].body, t, &w)
                  == CORE_OK);

            double rate = vec3_norm(w);
            CHECK(fabs(rate - SPIN[k].rate) / SPIN[k].rate < 2e-4);

            Quat q;
            CHECK(eph_body_orientation(eph, SPIN[k].body, t, &q) == CORE_OK);
            Vec3d pole = quat_rotate(q, vec3(0.0, 0.0, 1.0));
            CHECK(vec3_dot(w, pole) > 0.99 * rate);
        }

        /* A body with no orientation model does not turn - an answer, the
         * same way the identity quaternion is one. */
        Vec3d still;
        CHECK(eph_body_angular_velocity(eph, 6, t_begin, &still) == CORE_OK);
        CHECK(vec3_norm(still) == 0.0);

        /* And the same refusals as everything else that reads a fit. */
        Vec3d w;
        CHECK(eph_body_angular_velocity(eph, 0, t_begin - 1.0, &w)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_angular_velocity(eph, -1, t_begin, &w)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_angular_velocity(eph, 0, t_begin, NULL)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_angular_velocity(NULL, 0, t_begin, &w)
              == CORE_ERR_INVALID_ARG);
    }

    /* Same refusals as eph_body_state, and for the same reason: a fit
     * evaluated outside its interval is confident nonsense. "Not modelled"
     * is not one of them - it is an answer, checked above. */
    {
        Quat q;
        CHECK(eph_body_orientation(eph, 0, t_begin - 1.0, &q)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(eph, 0, t_end + 1.0, &q)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(eph, -1, t_begin, &q)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(eph, (int)N_ALL, t_begin, &q)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(eph, 0, t_begin, NULL)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(NULL, 0, t_begin, &q)
              == CORE_ERR_INVALID_ARG);

        /* Including for a body that carries no channels: a bad time has to
         * stay an error for every body, or the caller learns to trust an
         * answer that was never checked. */
        CHECK(eph_body_orientation(eph, 6, t_end + 1.0, &q)
              == CORE_ERR_INVALID_ARG);
        CHECK(eph_body_orientation(eph, 6, t_end, &q) == CORE_OK);
    }

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
        CHECK_BITS_EQ(second.max_orient_error_rad, report.max_orient_error_rad);
    }

    /* An asset built with no orientation at all: every body reads back as
     * the identity, and the file is smaller by exactly the block that is
     * missing. This is the configuration every caller written before K3b
     * still gets, since they memset their config to zero. */
    {
        EphBuildConfig none = default_config();
        none.orient_degree = 0;

        EphBuildReport plain;
        memset(&plain, 0, sizeof plain);
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &none, PATH_B,
                        &plain) == CORE_OK);

        size_t orient = (size_t)report.intervals * N_ORIENT * 4u * ORIENT_DEGREE
                      * sizeof(double);
        CHECK(plain.bytes_written + orient == report.bytes_written);
        CHECK_BITS_EQ(plain.max_orient_error_rad, 0.0);

        EphemerisCtx *bare = NULL;
        CHECK(eph_load(PATH_B, &bare) == CORE_OK);
        if (bare != NULL) {
            Quat q;
            CHECK(eph_body_orientation(bare, 3, 1234.5, &q) == CORE_OK);
            CHECK_BITS_EQ(q.w, 1.0);
            CHECK_BITS_EQ(q.x, 0.0);
            CHECK_BITS_EQ(q.y, 0.0);
            CHECK_BITS_EQ(q.z, 0.0);
            eph_free(bare);
        }
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
        write_corrupt(PATH_A, PATH_B, 44, &wrong_sentinel, sizeof wrong_sentinel);
        CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);

        /* A header saying there are no orientation coefficients, over bodies
         * that claim to carry some. Every read after the first such body
         * would land at the wrong offset, so this is a corrupt file and not
         * a body to quietly skip. The word at 24 is the orientation degree
         * (core/ephemeris.h's layout table). */
        unsigned no_orientation = 0u;
        write_corrupt(PATH_A, PATH_B, 24, &no_orientation,
                      sizeof no_orientation);
        CHECK(eph_load(PATH_B, &bad) == CORE_ERR_INVALID_ARG);

        /* And a flag that is neither 0 nor 1: the reader must not read it as
         * "true" and carry on. The flag of the first body sits after the
         * header and that body's name, mu, radius and flux. */
        unsigned not_a_flag = 7u;
        write_corrupt(PATH_A, PATH_B,
                      52 + EPH_NAME_SIZE + 3 * sizeof(double),
                      &not_a_flag, sizeof not_a_flag);
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

        /* One coefficient is not a quaternion channel, it is a constant -
         * refused rather than written as a rotation that never turns. Zero,
         * on the other hand, is the legitimate "no orientation" and is
         * exercised above. */
        bad = default_config();
        bad.orient_degree = 1;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);

        bad = default_config();
        bad.orient_degree = CHEB_FIT_MAX_N + 1;
        CHECK(eph_build(&system_config, initial, ALL_BODIES, &bad, PATH_B, NULL)
              == CORE_ERR_INVALID_ARG);
    }

    remove(PATH_A);
    remove(PATH_B);

    return TEST_RESULT();
}
