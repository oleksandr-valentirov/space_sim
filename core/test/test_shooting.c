/* Multiple shooting into the real ephemeris (ROADMAP C4).
 *
 * The measurement this file exists for: a CR3BP halo, carried into the full
 * model and corrected, becomes a genuinely ballistic trajectory that stays
 * beside L2 for months - no manoeuvres, no station-keeping, just a trajectory
 * that closes to a fraction of a metre at every patch point.
 *
 * It works for half the family and stalls for the other half, and the pattern
 * is the opposite of what one would guess. Both are asserted below, because a
 * limitation that is measured is worth as much as a success that is.
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "cr3bp.h"
#include "eph_build.h"
#include "field.h"
#include "frame.h"
#include "refdata.h"
#include "shooting.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define MAX_SAMPLES 256
#define DAY 86400.0

static const EphBodyInfo ALL_BODIES[] = {
    { "sun", 0.0, 0.0 },          { "mercury", 0.0, 0.0 },
    { "venus", 0.0, 0.0 },        { "earth", 0.0, 0.0 },
    { "moon", 0.0, 0.0 },         { "mars_bary", 0.0, 0.0 },
    { "jupiter_bary", 0.0, 0.0 }, { "saturn_bary", 0.0, 0.0 },
    { "uranus_bary", 0.0, 0.0 },  { "neptune_bary", 0.0, 0.0 },
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

#define EARTH 3
#define MOON  4

#define SPAN_DAYS 260.0
#define LEGS 8
#define MAX_PATCH 140

static const char *PATH = "build/test_shooting.eph";

static RefSample reference[N_ALL][MAX_SAMPLES];
static NBodySystem system_config;
static State initial[NBODY_MAX];
static RefHalo halo[16];
static size_t n_halo;
static double halo_mu;

static State patch[MAX_PATCH];
static State guess[MAX_PATCH];
static double times[MAX_PATCH];
static double workspace[SHOOTING_WORKSPACE(MAX_PATCH)];

static EphemerisCtx *eph;
static double mean_length;
static double mean_rate;

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
                             &n_halo) == CORE_OK
           && refdata_load_scalar("data/jpl_halo/mu.txt", &halo_mu) == CORE_OK;
}

/* Lay out the reference: one revolution of the CR3BP orbit, sampled, then
 * repeated - and repeated rather than propagated, which matters more than it
 * looks. Orbit 1151 multiplies a perturbation by 594 per revolution, so
 * integrating it for sixteen revolutions to lay out patch points destroys the
 * reference long before the corrector ever sees it: 594^16 times the
 * integrator's own error is not a small number. The orbit is periodic, so one
 * period is all there is to compute. */
static int lay_out(const RefHalo *h, double revolutions, size_t *out_n)
{
    Cr3bpCtx ctx = { halo_mu };

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-13;
    cfg.max_steps = 1000000;

    double step = h->period / (double)LEGS;

    State cycle[LEGS];
    State q = h->s;
    q.t = 0.0;

    for (int i = 0; i < LEGS; i++) {
        cycle[i] = q;

        Dop853State st;
        memset(&st, 0, sizeof st);
        State next;
        if (dop853_integrate(accel_cr3bp, &ctx, &q, q.t + step, &cfg, &st,
                             &next) != CORE_OK) {
            return 0;
        }
        q = next;
    }

    size_t want = (size_t)(revolutions * (double)LEGS) + 1;
    if (want > MAX_PATCH) {
        want = MAX_PATCH;
    }

    double t = 0.0;
    size_t n = 0;

    while (n < want) {
        SynodicFrame frame;
        if (frame_synodic(eph, EARTH, MOON, t, &frame) != CORE_OK) {
            break;
        }

        State dimensionless = cycle[n % LEGS];
        dimensionless.t = 0.0;

        times[n] = t;
        frame_to_inertial(&frame, &dimensionless, &patch[n]);
        guess[n] = patch[n];
        n++;

        t += step / frame.rate;
    }

    *out_n = n;
    return n >= 3;
}

/* Largest distance a patch point moved, and largest distance from L2, both in
 * length units. */
static void survey(size_t n, double *moved, double *from_l2)
{
    *moved = 0.0;
    *from_l2 = 0.0;

    for (size_t i = 0; i < n; i++) {
        double d = vec3_distance(patch[i].r, guess[i].r) / mean_length;
        if (d > *moved) {
            *moved = d;
        }

        SynodicFrame frame;
        if (frame_synodic(eph, EARTH, MOON, times[i], &frame) != CORE_OK) {
            continue;
        }

        State q;
        frame_from_inertial(&frame, &patch[i], &q);

        Vec3d l2;
        if (cr3bp_lagrange(frame.mu, 2, &l2) != CORE_OK) {
            continue;
        }

        double away = vec3_distance(q.r, l2);
        if (away > *from_l2) {
            *from_l2 = away;
        }
    }
}

static ShootingConfig shooting_config(int max_iterations)
{
    ShootingConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2;
    cfg.continuity_m = 1.0;
    cfg.length_scale = mean_length;
    cfg.speed_scale = mean_length * mean_rate;
    cfg.max_iterations = max_iterations;
    return cfg;
}

int main(void)
{
    if (!load_inputs()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    /* Inside the CR3BP first, where the answer is known: sample a published
     * halo, displace every patch point but the first, and correct. It must
     * come back. This separates the linear algebra from the ephemeris - if
     * this fails, nothing about the real model is worth looking at. */
    {
        Cr3bpCtx ctx = { halo_mu };

        Dop853Config cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-13;
        cfg.max_steps = 1000000;

        const size_t n = 17;
        double step = halo[2].period / (double)(n - 1);

        State q = halo[2].s;
        q.t = 0.0;
        for (size_t i = 0; i < n; i++) {
            times[i] = step * (double)i;
            patch[i] = q;
            patch[i].t = times[i];
            guess[i] = patch[i];

            if (i + 1 < n) {
                Dop853State st;
                memset(&st, 0, sizeof st);
                State next;
                CHECK(dop853_integrate(accel_cr3bp, &ctx, &q, times[i] + step,
                                       &cfg, &st, &next) == CORE_OK);
                q = next;
            }
        }

        /* guess[] still holds the true halo; patch[] is displaced from it. */
        for (size_t i = 1; i < n; i++) {
            patch[i].r.x += 1e-4;
            patch[i].v.y -= 1e-4;
        }

        ShootingConfig sc;
        memset(&sc, 0, sizeof sc);
        sc.tol_m = 1e-13;
        sc.continuity_m = 1e-11;
        sc.length_scale = 1.0;   /* already dimensionless */
        sc.speed_scale = 1.0;
        sc.max_iterations = 20;

        ShootingReport report;
        CHECK(shoot_multiple(accel_cr3bp_var, &ctx, patch, times, n, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_OK);

        /* Measured: three iterations to a gap of 4.4e-14. */
        CHECK(report.iterations <= 5);
        CHECK(report.worst_position_gap < 1e-11);
        CHECK(report.worst_velocity_gap < 1e-9);

        /* And it landed beside the orbit it was displaced from rather than on
         * some unrelated continuous trajectory. Not exactly on it, and that is
         * the correct behaviour: the corrector is asked for the continuous
         * trajectory nearest to what it was GIVEN, which is the displaced
         * points, not the halo they came from. So the right test is that the
         * answer is no further from the halo than the displacement was.
         * Measured worst distance 1.1e-4 against a displacement of 1e-4. */
        double worst = 0.0;
        for (size_t i = 0; i < n; i++) {
            double d = vec3_distance(patch[i].r, guess[i].r);
            if (d > worst) {
                worst = d;
            }
        }
        CHECK(worst < 3e-4);
    }

    /* Now the real model. */
    EphBuildConfig build;
    memset(&build, 0, sizeof build);
    build.t_begin = 0.0;
    build.t_end = SPAN_DAYS * DAY;
    build.interval_seconds = 8.0 * DAY;
    build.degree = 14;
    build.tol_m = 1.0;

    EphBuildReport build_report;
    memset(&build_report, 0, sizeof build_report);
    CHECK(eph_build(&system_config, initial, ALL_BODIES, &build, PATH,
                    &build_report) == CORE_OK);

    CHECK(eph_load(PATH, &eph) == CORE_OK);
    if (eph == NULL) {
        return EXIT_FAILURE;
    }

    {
        double sum_length = 0.0, sum_rate = 0.0;
        int count = 0;
        for (double t = 0.0; t < SPAN_DAYS * DAY; t += DAY) {
            SynodicFrame f;
            if (frame_synodic(eph, EARTH, MOON, t, &f) == CORE_OK) {
                sum_length += f.length;
                sum_rate += f.rate;
                count++;
            }
        }
        CHECK(count > 100);
        mean_length = sum_length / (double)count;
        mean_rate = sum_rate / (double)count;
    }

    FieldCtx field;
    CHECK(field_all_bodies(eph, &field) == CORE_OK);

    /* Orbit 1151, sixteen revolutions - 233 days of model time.
     *
     * Measured: five iterations, worst position discontinuity 2.3e-2 m, patch
     * points moved 0.0097 length units (3.7e6 m) from the CR3BP guess, and
     * the whole trajectory stayed within 0.123 length units of L2. That is a
     * ballistic trajectory in the full ten-body model, beside L2, for eight
     * months, and it is what ROADMAP C4 asks for. */
    {
        size_t n = 0;
        CHECK(lay_out(&halo[3], 16.0, &n));
        CHECK(n >= 100);

        ShootingConfig sc = shooting_config(30);
        ShootingReport report;
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_OK);

        CHECK(report.iterations <= 15);
        CHECK(report.worst_position_gap < 1.0);
        CHECK(report.worst_velocity_gap < 1e-3);

        double moved, from_l2;
        survey(n, &moved, &from_l2);
        CHECK(moved < 0.05);
        CHECK(from_l2 < 0.3);
        CHECK(times[n - 1] > 200.0 * DAY);
    }

    /* Orbit 767, eight revolutions - 109 days. Measured: six iterations,
     * gap 5.8e-4 m, moved 0.0085 units, within 0.166 of L2. */
    {
        size_t n = 0;
        CHECK(lay_out(&halo[2], 8.0, &n));

        ShootingConfig sc = shooting_config(30);
        ShootingReport report;
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_OK);

        CHECK(report.worst_position_gap < 1.0);

        double moved, from_l2;
        survey(n, &moved, &from_l2);
        CHECK(moved < 0.05);
        CHECK(from_l2 < 0.3);
    }

    /* And the half of the family it does not manage.
     *
     * Orbit 0 stalls at four revolutions: the residual falls to about 1e6 m
     * and stops, and stays there through two hundred iterations and through
     * four times as many patch points. It has not wandered off - the survey
     * shows it still within 0.23 length units of L2 - it simply cannot close.
     *
     * The pattern across the four orbits is the opposite of the obvious one:
     *
     *   orbit 1151  eigenvalue 594   amplitude 0.097   converges, 16+ revs
     *   orbit 767   eigenvalue 150   amplitude 0.159   converges, 16 revs
     *   orbit 383   eigenvalue 3.1   amplitude 0.195   stalls past 2 revs
     *   orbit 0     eigenvalue 1.19  amplitude 0.202   stalls past 2 revs
     *
     * The more violently unstable the orbit, the more readily it corrects.
     * Two explanations fit and these four orbits cannot separate them, since
     * amplitude and instability move together along the family: a sensitive
     * transition matrix gives the corrector more leverage per metre of patch
     * movement, and a smaller halo sits deeper in the region where the CR3BP
     * is a good approximation at all. Recorded as an observation, not as a
     * mechanism.
     *
     * Asserted as a failure so that a future change which fixes it will be
     * noticed rather than passing silently. */
    {
        size_t n = 0;
        CHECK(lay_out(&halo[0], 4.0, &n));

        ShootingConfig sc = shooting_config(30);
        ShootingReport report;
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_ERR_TOLERANCE_NOT_MET);

        CHECK(report.worst_position_gap > 1.0);

        double moved, from_l2;
        survey(n, &moved, &from_l2);
        CHECK(from_l2 < 0.5);
    }

    /* Buffers and arguments. */
    {
        size_t n = 0;
        CHECK(lay_out(&halo[3], 2.0, &n));

        ShootingConfig sc = shooting_config(30);
        ShootingReport report;

        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &sc,
                             workspace, SHOOTING_WORKSPACE(n) - 1, &report)
              == CORE_ERR_BUFFER_TOO_SMALL);
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, 1, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_ERR_INVALID_ARG);
        CHECK(shoot_multiple(NULL, &field, patch, times, n, &sc, workspace,
                             sizeof workspace / sizeof workspace[0], &report)
              == CORE_ERR_INVALID_ARG);

        ShootingConfig bad = sc;
        bad.length_scale = 0.0;
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &bad,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_ERR_INVALID_ARG);
    }

    eph_free(eph);
    remove(PATH);

    return TEST_RESULT();
}
