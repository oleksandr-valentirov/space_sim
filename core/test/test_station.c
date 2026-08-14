/* What it costs to stay on a halo orbit (ROADMAP C4).
 *
 * The reference this flies is the corrected trajectory from
 * core/test/test_shooting.c - ballistic in the full ten-body model, so a
 * perfectly injected vessel needs nothing at all. The budget is therefore the
 * price of being somewhere else than you meant to be, which is the only
 * reason real missions on halo orbits burn fuel.
 *
 * Two results are worth the file. The cost scales linearly with the injection
 * error, which says the controller is working in the regime it was linearised
 * for. And the cost depends violently on how far ahead the controller aims -
 * by nine orders of magnitude between the worst choice and a sensible one -
 * which is the kind of thing that is obvious once measured and invisible
 * before.
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "cr3bp.h"
#include "eph_build.h"
#include "field.h"
#include "frame.h"
#include "refdata.h"
#include "shooting.h"
#include "station.h"
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

static const char *PATH = "build/test_station.eph";

static RefSample reference_data[N_ALL][MAX_SAMPLES];
static NBodySystem system_config;
static State initial[NBODY_MAX];
static RefHalo halo[16];
static size_t n_halo;
static double halo_mu;

static State patch[MAX_PATCH];
static double times[MAX_PATCH];
static double workspace[SHOOTING_WORKSPACE(MAX_PATCH)];

static EphemerisCtx *eph;
static FieldCtx field;

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
        if (refdata_load_vectors(path, reference_data[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            return 0;
        }
        system_config.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i].name);
        initial[i] = reference_data[i][0].s;
    }

    return refdata_load_halo("data/jpl_halo/halo_l2_south.csv", halo, 16,
                             &n_halo) == CORE_OK
           && refdata_load_scalar("data/jpl_halo/mu.txt", &halo_mu) == CORE_OK;
}

/* One revolution of the CR3BP orbit, sampled and then repeated. See the note
 * in core/test/test_shooting.c for why it is repeated rather than propagated. */
static size_t lay_out(const RefHalo *h, double revolutions)
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
        n++;

        t += step / frame.rate;
    }

    return n;
}

static StationConfig station_config(int horizon)
{
    StationConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2;
    cfg.target_tol_m = 1.0;
    cfg.control_interval = 4;
    cfg.horizon = horizon;
    cfg.max_iterations = 15;
    return cfg;
}

/* Fly with an injection error of `error` metres along x. */
static CoreResult flight(size_t n, double error, int horizon,
                         StationReport *out)
{
    State start = patch[0];
    start.r.x += error;

    StationConfig cfg = station_config(horizon);
    return station_keep(accel_field_var, &field, patch, times, n, &start, &cfg,
                        out);
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

    EphBuildReport build_report;
    memset(&build_report, 0, sizeof build_report);
    CHECK(eph_build(&system_config, initial, ALL_BODIES, &build, PATH,
                    &build_report) == CORE_OK);

    CHECK(eph_load(PATH, &eph) == CORE_OK);
    if (eph == NULL) {
        return EXIT_FAILURE;
    }
    CHECK(field_all_bodies(eph, &field) == CORE_OK);

    double mean_length = 0.0, mean_rate = 0.0;
    {
        int count = 0;
        for (double t = 0.0; t < SPAN_DAYS * DAY; t += DAY) {
            SynodicFrame f;
            if (frame_synodic(eph, EARTH, MOON, t, &f) == CORE_OK) {
                mean_length += f.length;
                mean_rate += f.rate;
                count++;
            }
        }
        CHECK(count > 100);
        mean_length /= (double)count;
        mean_rate /= (double)count;
    }

    /* The reference: sixteen revolutions of orbit 1151, made continuous. */
    size_t n = lay_out(&halo[3], 16.0);
    CHECK(n >= 100);

    {
        ShootingConfig sc;
        memset(&sc, 0, sizeof sc);
        sc.tol_m = 1e-2;
        sc.continuity_m = 1.0;
        sc.length_scale = mean_length;
        sc.speed_scale = mean_length * mean_rate;
        sc.max_iterations = 30;

        ShootingReport report;
        CHECK(shoot_multiple(accel_field_var, &field, patch, times, n, &sc,
                             workspace, sizeof workspace / sizeof workspace[0],
                             &report) == CORE_OK);
        CHECK(report.worst_position_gap < 1.0);
    }

    CHECK(times[n - 1] > 200.0 * DAY);

    /* A perfect injection costs nothing, which is the statement that the
     * reference really is ballistic rather than merely close. Measured
     * 1e-4 m/s over 233 days, which is the targeter chasing the metre of
     * continuity the shooting left behind. */
    {
        StationReport report;
        CHECK(flight(n, 0.0, 4, &report) == CORE_OK);
        CHECK(report.completed == 1);
        CHECK(report.per_year < 1e-2);
        CHECK(report.worst_offset_m < 100.0);
    }

    /* And an imperfect one costs in proportion to how imperfect it was.
     * Measured, aiming half a revolution ahead:
     *
     *    1 km error   0.035 m/s per year, staying within 2.5 km
     *   10 km error   0.348 m/s per year, staying within 25 km
     *
     * Linear to better than a percent, which says the controller is working
     * in the regime its linearisation assumes, and the offset scaling the
     * same way says the vessel is being held rather than dragged. */
    {
        StationReport small, large;
        CHECK(flight(n, 1.0e3, 4, &small) == CORE_OK);
        CHECK(flight(n, 1.0e4, 4, &large) == CORE_OK);

        CHECK(small.completed == 1);
        CHECK(large.completed == 1);

        CHECK(small.per_year > 1e-3);
        CHECK(small.per_year < 1.0);

        double ratio = large.per_year / small.per_year;
        CHECK(ratio > 8.0);
        CHECK(ratio < 12.0);

        double offset_ratio = large.worst_offset_m / small.worst_offset_m;
        CHECK(offset_ratio > 8.0);
        CHECK(offset_ratio < 12.0);

        /* Single digits of metres per second per year is what real halo
         * missions budget. Being far below it is expected here: the reference
         * is exactly ballistic and the only error is a one-off injection,
         * with no navigation uncertainty and no unmodelled forces. */
        CHECK(large.per_year < 5.0);
    }

    /* The horizon, which is the whole design of the controller.
     *
     * Aiming at the very next patch point is catastrophic: measured 5.5e9 m/s
     * before it gives up, because forcing the vessel onto the reference an
     * eighth of a revolution away also forces it to arrive with whatever
     * velocity that demands, and near L2 that velocity is enormous. Half a
     * revolution ahead costs 1e-4. Nine orders of magnitude for a single
     * integer, and nothing about the equations hints at it. */
    {
        StationReport near, sensible;
        CHECK(flight(n, 0.0, 1, &near) == CORE_OK);
        CHECK(flight(n, 0.0, 4, &sensible) == CORE_OK);

        CHECK(near.total_dv > 1e6);
        CHECK(sensible.total_dv < 1.0);

        /* Aiming too far ahead fails differently, and it is worth knowing
         * that it fails rather than merely costs more: over two revolutions
         * the transition matrix of an orbit with eigenvalue 594 is too
         * sensitive for the targeter to converge, so it stops early. */
        StationReport distant;
        CHECK(flight(n, 0.0, 16, &distant) == CORE_OK);
        CHECK(distant.completed == 0);
    }

    /* Argument checking. */
    {
        StationReport report;
        StationConfig cfg = station_config(4);

        CHECK(station_keep(NULL, &field, patch, times, n, &patch[0], &cfg,
                           &report) == CORE_ERR_INVALID_ARG);
        CHECK(station_keep(accel_field_var, &field, patch, times, 1, &patch[0],
                           &cfg, &report) == CORE_ERR_INVALID_ARG);
        CHECK(station_keep(accel_field_var, &field, patch, times, n, NULL,
                           &cfg, &report) == CORE_ERR_INVALID_ARG);

        cfg.tol_m = 0.0;
        CHECK(station_keep(accel_field_var, &field, patch, times, n, &patch[0],
                           &cfg, &report) == CORE_ERR_INVALID_ARG);
    }

    eph_free(eph);
    remove(PATH);

    return TEST_RESULT();
}
