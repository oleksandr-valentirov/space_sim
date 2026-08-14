/* The synodic frame of the real Earth and Moon, and what a CR3BP orbit does
 * when it is put into it (ROADMAP C4).
 *
 * The second half of this file is the measurement C4 is really about. A halo
 * orbit is periodic in a model where the primaries are a fixed distance apart
 * and turn at a constant rate. The Earth and Moon do neither - the separation
 * swings by a tenth over a month, and the angular rate with it - so the
 * question is not whether a converted orbit is periodic, which it cannot be,
 * but how long it stays anywhere near L2 before the mismatch throws it out.
 * That number is the baseline every correction scheme is measured against.
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "cr3bp.h"
#include "eph_build.h"
#include "field.h"
#include "frame.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

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

#define EARTH 3
#define MOON  4

#define SPAN_DAYS 120.0

static const char *PATH = "build/test_frame.eph";

static RefSample reference[N_ALL][MAX_SAMPLES];
static NBodySystem system_config;
static State initial[NBODY_MAX];
static RefHalo halo[16];
static size_t n_halo;

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
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i].name);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            return 0;
        }
        system_config.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i].name);
        initial[i] = reference[i][0].s;
    }

    return refdata_load_halo("data/jpl_halo/halo_l2_south.csv", halo, 16,
                             &n_halo) == CORE_OK;
}

/* Revolutions of the CR3BP period before the vessel is further than one
 * length unit from L2, or -1 if it never is within the span. */
static double revolutions_until_escape(EphemerisCtx *eph, const RefHalo *h,
                                       double epoch, double t_limit)
{
    SynodicFrame frame;
    if (frame_synodic(eph, EARTH, MOON, epoch, &frame) != CORE_OK) {
        return -2.0;
    }

    State vessel;
    frame_to_inertial(&frame, &h->s, &vessel);

    FieldCtx field;
    if (field_all_bodies(eph, &field) != CORE_OK) {
        return -2.0;
    }

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2;
    cfg.max_steps = 20000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    double period = h->period / frame.rate;

    for (int k = 1; k <= 400; k++) {
        double t = epoch + period * (double)k / 16.0;
        if (t > t_limit) {
            return -1.0;
        }

        State next;
        if (dop853_integrate(accel_field, &field, &vessel, t, &cfg, &st, &next)
            != CORE_OK) {
            return -2.0;
        }
        vessel = next;

        SynodicFrame now;
        if (frame_synodic(eph, EARTH, MOON, t, &now) != CORE_OK) {
            return -2.0;
        }

        State q;
        frame_from_inertial(&now, &vessel, &q);

        Vec3d l2;
        if (cr3bp_lagrange(now.mu, 2, &l2) != CORE_OK) {
            return -2.0;
        }

        if (vec3_distance(q.r, l2) > 1.0) {
            return (double)k / 16.0;
        }
    }

    return -1.0;
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

    /* The frame itself. */
    {
        SynodicFrame f;
        CHECK(frame_synodic(eph, EARTH, MOON, 0.0, &f) == CORE_OK);

        /* Orthonormal to the last bit or two: measured 1.1e-16 on the norms
         * and 4.2e-17 on the dot products. */
        CHECK(fabs(vec3_norm(f.x) - 1.0) < 1e-15);
        CHECK(fabs(vec3_norm(f.y) - 1.0) < 1e-15);
        CHECK(fabs(vec3_norm(f.z) - 1.0) < 1e-15);
        CHECK(fabs(vec3_dot(f.x, f.y)) < 1e-15);
        CHECK(fabs(vec3_dot(f.x, f.z)) < 1e-15);
        CHECK(fabs(vec3_dot(f.y, f.z)) < 1e-15);

        /* And the scales are the real ones, not the CR3BP's constants. The
         * separation swings between 3.63e8 and 4.05e8 m over the span, so
         * the length unit a converted orbit is stretched by depends on the
         * epoch it is converted at - measured 4.02e8 m at t = 0 against the
         * catalogue's own unit of 3.897e8 m, a 3% difference before anything
         * has been integrated at all. */
        CHECK(f.length > 3.5e8);
        CHECK(f.length < 4.1e8);
        CHECK(fabs(f.length_rate) < 100.0);

        double period_days = 2.0 * 3.14159265358979323846 / f.rate / DAY;
        CHECK(period_days > 24.0);
        CHECK(period_days < 32.0);

        /* The mass ratio comes from the asset, and it is not the catalogue's:
         * they differ in the eighth digit, as core/test/test_halo.c records.
         * The frame must use the bodies it actually has. */
        CHECK(fabs(f.mu - 0.0121505842695) < 1e-12);
    }

    /* The two bodies land where the CR3BP says they should, and they sit
     * still: measured 1e-14 in position and 1.2e-14 in dimensionless speed.
     *
     * The speed being zero is a construction check rather than a physical
     * one - the omega and stretch terms are built to cancel exactly for a
     * point on the line between the bodies - but it is the check that catches
     * a factor of L or of rate in the wrong place. */
    {
        SynodicFrame f;
        CHECK(frame_synodic(eph, EARTH, MOON, 20.0 * DAY, &f) == CORE_OK);

        State body, q;
        CHECK(eph_body_state(eph, EARTH, f.t, &body) == CORE_OK);
        frame_from_inertial(&f, &body, &q);
        CHECK(fabs(q.r.x + f.mu) < 1e-13);
        CHECK(fabs(q.r.y) < 1e-13);
        CHECK(fabs(q.r.z) < 1e-13);
        CHECK(vec3_norm(q.v) < 1e-12);

        CHECK(eph_body_state(eph, MOON, f.t, &body) == CORE_OK);
        frame_from_inertial(&f, &body, &q);
        CHECK(fabs(q.r.x - (1.0 - f.mu)) < 1e-13);
        CHECK(fabs(q.r.y) < 1e-13);
        CHECK(fabs(q.r.z) < 1e-13);
        CHECK(vec3_norm(q.v) < 1e-12);
    }

    /* Round trip, on a state that is nowhere near the plane or the axis. */
    {
        SynodicFrame f;
        CHECK(frame_synodic(eph, EARTH, MOON, 33.0 * DAY, &f) == CORE_OK);

        State q = halo[2].s;
        State inertial, back;
        frame_to_inertial(&f, &q, &inertial);
        frame_from_inertial(&f, &inertial, &back);

        CHECK(vec3_distance(q.r, back.r) < 1e-13);
        CHECK(vec3_norm(vec3_sub(q.v, back.v)) < 1e-13);
        CHECK_BITS_EQ(inertial.t, f.t);

        /* Sanity on the magnitudes: a halo near L2 is about 4.6e8 m from the
         * barycentre and moving at about 900 m/s relative to it. */
        CHECK(vec3_distance(inertial.r, f.origin) > 4.0e8);
        CHECK(vec3_distance(inertial.r, f.origin) < 5.5e8);
        CHECK(vec3_norm(vec3_sub(inertial.v, f.origin_rate)) > 500.0);
        CHECK(vec3_norm(vec3_sub(inertial.v, f.origin_rate)) < 1500.0);
    }

    /* The size of the approximation in the header, measured rather than
     * argued. omega is built to leave z fixed, so the true rotation of z is
     * exactly what is left out. Measured |dz/dt| between 2.4e-10 and 2.9e-9
     * rad/s across the span, against a frame rate near 2.6e-6 - so the
     * neglected term is between 1e-4 and 1.2e-3 of the rotation being
     * modelled. That is small enough to leave out and large enough that it
     * should be written down. */
    {
        double worst_ratio = 0.0;

        for (double t = 10.0 * DAY; t < 110.0 * DAY; t += 10.0 * DAY) {
            SynodicFrame before, at, after;
            const double dt = 3600.0;

            CHECK(frame_synodic(eph, EARTH, MOON, t - dt, &before) == CORE_OK);
            CHECK(frame_synodic(eph, EARTH, MOON, t, &at) == CORE_OK);
            CHECK(frame_synodic(eph, EARTH, MOON, t + dt, &after) == CORE_OK);

            Vec3d dz = vec3_scale(vec3_sub(after.z, before.z), 1.0 / (2.0 * dt));
            double ratio = vec3_norm(dz) / at.rate;
            if (ratio > worst_ratio) {
                worst_ratio = ratio;
            }
        }

        CHECK(worst_ratio > 1e-6);    /* it is there */
        CHECK(worst_ratio < 1e-2);    /* and it is small */
    }

    /* The baseline. Every catalogue orbit, converted at two epochs, leaves
     * the neighbourhood of L2 within a handful of revolutions.
     *
     * Measured, revolutions to escape:
     *
     *   orbit 0    (eigenvalue 1.19)   3.31 and 2.69
     *   orbit 383  (eigenvalue 3.06)   3.69 and 3.88
     *   orbit 767  (eigenvalue 150)    1.31 and 1.25
     *   orbit 1151 (eigenvalue 594)    1.12 and 1.06
     *
     * The ordering follows the instability for the three unstable orbits and
     * breaks for orbit 0, which is the least unstable of the four and does
     * not last longest. That is the finding: what removes a converted orbit
     * is not only its own instability but the mismatch between the models,
     * and for a nearly stable orbit the mismatch is all of it. Correction is
     * therefore not optional for any member of the family, which is what
     * makes the next step necessary rather than an improvement. */
    {
        double epochs[2] = { 0.0, 30.0 * DAY };

        for (size_t i = 0; i < 4; i++) {
            for (int e = 0; e < 2; e++) {
                double revolutions = revolutions_until_escape(
                    eph, &halo[i], epochs[e], SPAN_DAYS * DAY);

                CHECK(revolutions > 0.5);
                CHECK(revolutions < 8.0);
            }
        }
    }

    /* Argument checking. */
    {
        SynodicFrame f;
        CHECK(frame_synodic(NULL, EARTH, MOON, 0.0, &f) == CORE_ERR_INVALID_ARG);
        CHECK(frame_synodic(eph, EARTH, MOON, 0.0, NULL)
              == CORE_ERR_INVALID_ARG);
        CHECK(frame_synodic(eph, EARTH, EARTH, 0.0, &f)
              == CORE_ERR_INVALID_ARG);
        CHECK(frame_synodic(eph, EARTH, MOON, -DAY, &f)
              == CORE_ERR_INVALID_ARG);
        CHECK(frame_synodic(eph, EARTH, MOON, (SPAN_DAYS + 1.0) * DAY, &f)
              == CORE_ERR_INVALID_ARG);
    }

    eph_free(eph);
    remove(PATH);

    return TEST_RESULT();
}
