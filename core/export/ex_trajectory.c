/* Export: the L2 mission in the real ephemeris (Milestone 0 delivery).
 *
 * Two files:
 *
 *   halo_inertial.csv  the shot trajectory, sampled, with Earth and the Moon
 *                      beside it and the vessel also given in the synodic
 *                      frame of the instant
 *   station.csv        what it costs to hold that trajectory, against the
 *                      controller's aim horizon
 *
 * The first is the picture Milestone 0 was built to produce. Everything up to
 * C4 is visible in it at once: a CR3BP orbit carried into the real Earth-Moon
 * system (frame.h), made continuous there by multiple shooting (shooting.h),
 * flying in the field of ten point masses read from a cooked asset (field.h).
 * Plotted inertially it is a curve that follows the Moon around the Earth;
 * plotted in the synodic frame it closes into the halo it came from. Neither
 * view alone shows that both things are true.
 *
 * The second is the sentence from station.h as a curve: aiming at the next
 * patch point forces the vessel onto the reference immediately and costs a
 * great deal, aiming further ahead lets the dynamics do the work, and the cost
 * falls steeply and then flattens.
 *
 * The setup deliberately matches core/scenario/sc_trajectory.c - same
 * catalogue orbit, same eight legs per revolution, same committed asset - so a
 * plot that looks wrong can be checked against a hash that is known good.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "csv.h"
#include "field.h"
#include "frame.h"
#include "refdata.h"
#include "shooting.h"
#include "station.h"

#include <string.h>

#define ASSET "data/fixture/earth_moon.eph"

/* Indices into the asset, whose body order is the cooker's (cook_fixture.c). */
#define EARTH 3
#define MOON  4

/* Catalogue orbit 1151, the strongly unstable one: lambda 594 per revolution.
 * The interesting case for both files - a nearly stable orbit would make
 * station-keeping look free. */
#define ORBIT 3

#define LEGS 8
#define PATCHES 57              /* seven revolutions plus the closing point */
#define SAMPLES_PER_LEG 24

#define INJECTION_ERROR 1.0e3   /* metres, along x */
#define MAX_HORIZON 12

static RefHalo halo[16];
static size_t n_halo;
static double halo_mu;

static EphemerisCtx *eph;
static FieldCtx field;

static State patch[PATCHES];
static double times[PATCHES];
static double workspace[SHOOTING_WORKSPACE(PATCHES)];

static double length_scale, rate_scale;

/* One revolution of patch points in the CR3BP, then repeated around the real
 * system. Repeated rather than propagated for seven revolutions because the
 * orbit's own instability would destroy the reference long before the end -
 * the same reason core/test/test_shooting.c gives. */
static int lay_out(void)
{
    Cr3bpCtx ctx = { halo_mu };

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-13;
    cfg.max_steps = 1000000;

    double step = halo[ORBIT].period / (double)LEGS;

    State cycle[LEGS];
    State q = halo[ORBIT].s;
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

    double t = 0.0;
    double sum_length = 0.0, sum_rate = 0.0;

    for (int i = 0; i < PATCHES; i++) {
        SynodicFrame frame;
        if (frame_synodic(eph, EARTH, MOON, t, &frame) != CORE_OK) {
            return 0;
        }

        State dimensionless = cycle[i % LEGS];
        dimensionless.t = 0.0;

        times[i] = t;
        frame_to_inertial(&frame, &dimensionless, &patch[i]);

        sum_length += frame.length;
        sum_rate += frame.rate;

        t += step / frame.rate;
    }

    length_scale = sum_length / (double)PATCHES;
    rate_scale = sum_rate / (double)PATCHES;
    return 1;
}

static int shoot(void)
{
    ShootingConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2;
    cfg.continuity_m = 1.0;
    cfg.length_scale = length_scale;
    cfg.speed_scale = length_scale * rate_scale;
    cfg.max_iterations = 30;

    ShootingReport report;
    if (shoot_multiple(accel_field_var, &field, patch, times, PATCHES, &cfg,
                       workspace, sizeof workspace / sizeof workspace[0],
                       &report) != CORE_OK) {
        fprintf(stderr, "ex_trajectory: multiple shooting did not converge\n");
        return 0;
    }

    printf("  shooting: %d iterations, worst gap %.3g m, %.3g m/s\n",
           report.iterations, report.worst_position_gap,
           report.worst_velocity_gap);
    return 1;
}

static int export_trajectory(void)
{
    Csv c;
    /* Velocity goes out too, though no plot here uses it. A trajectory export
     * that carries half a state vector is not a trajectory export - anything
     * that wants to check this against another tool, or to build on it, needs
     * both halves, and three columns cost nothing. */
    if (!csv_open(&c, "build/csv/halo_inertial.csv",
                  "t,days,x,y,z,vx,vy,vz,earth_x,earth_y,earth_z,"
                  "moon_x,moon_y,moon_z,sx,sy,sz")) {
        return 0;
    }

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2;
    cfg.max_steps = 1000000;

    /* Each leg is sampled from its own patch point, never from the previous
     * leg's end. That is what the trajectory IS after multiple shooting: a
     * sequence of legs continuous to a metre, not one propagation. Sampling it
     * as one propagation would show the 594-per-revolution instability eating
     * the metre, which would be a picture of the wrong thing. */
    for (int i = 0; i < PATCHES - 1; i++) {
        State s = patch[i];
        Dop853State st;
        memset(&st, 0, sizeof st);

        int last = (i == PATCHES - 2) ? SAMPLES_PER_LEG : SAMPLES_PER_LEG - 1;

        for (int k = 0; k <= last; k++) {
            double t = times[i] + (times[i + 1] - times[i])
                                  * (double)k / (double)SAMPLES_PER_LEG;

            if (k > 0) {
                State out;
                if (dop853_integrate(accel_field, &field, &s, t, &cfg, &st,
                                     &out) != CORE_OK) {
                    return 0;
                }
                s = out;
            }

            State earth, moon;
            SynodicFrame frame;
            if (eph_body_state(eph, EARTH, t, &earth) != CORE_OK
                || eph_body_state(eph, MOON, t, &moon) != CORE_OK
                || frame_synodic(eph, EARTH, MOON, t, &frame) != CORE_OK) {
                return 0;
            }

            State synodic;
            frame_from_inertial(&frame, &s, &synodic);

            csv_row(&c, 17, t, t / 86400.0,
                    s.r.x, s.r.y, s.r.z,
                    s.v.x, s.v.y, s.v.z,
                    earth.r.x, earth.r.y, earth.r.z,
                    moon.r.x, moon.r.y, moon.r.z,
                    synodic.r.x, synodic.r.y, synodic.r.z);
        }
    }

    if (field.failed) {
        fprintf(stderr, "ex_trajectory: the field ran off the ephemeris\n");
        return 0;
    }

    return csv_close(&c);
}

static int export_station(void)
{
    Csv c;
    if (!csv_open(&c, "build/csv/station.csv",
                  "horizon,completed,dv_per_year,total_dv,largest_dv,"
                  "manoeuvres,worst_offset_m,days")) {
        return 0;
    }

    for (int horizon = 1; horizon <= MAX_HORIZON; horizon++) {
        StationConfig cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-2;
        cfg.target_tol_m = 1.0;
        cfg.control_interval = 4;
        cfg.horizon = horizon;
        cfg.max_iterations = 15;

        State start = patch[0];
        start.r.x += INJECTION_ERROR;

        StationReport report;
        if (station_keep(accel_field_var, &field, patch, times, PATCHES,
                         &start, &cfg, &report) != CORE_OK) {
            fprintf(stderr, "ex_trajectory: station-keeping failed at "
                            "horizon %d\n", horizon);
            return 0;
        }

        /* completed is a column rather than something to infer, because a run
         * that stopped early still reports a cost and that cost is not a
         * budget for the flight - it is the price of the part that was flown
         * before the targeter gave up. Aiming too far ahead fails this way
         * rather than merely costing more: over two revolutions the transition
         * matrix of an orbit with eigenvalue 594 is too sensitive to invert
         * (core/test/test_station.c). The failure is not monotone in the
         * horizon either, which is worth seeing rather than smoothing away. */
        csv_row(&c, 8, (double)horizon, (double)report.completed,
                report.per_year, report.total_dv, report.largest_dv,
                (double)report.manoeuvres, report.worst_offset_m,
                report.flown / 86400.0);
    }

    return csv_close(&c);
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", halo, 16,
                          &n_halo) != CORE_OK
        || refdata_load_scalar("data/jpl_halo/mu.txt", &halo_mu) != CORE_OK
        || n_halo <= ORBIT) {
        fprintf(stderr, "ex_trajectory: cannot read data/jpl_halo/\n");
        return 1;
    }

    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "ex_trajectory: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds "
                        "it\n");
        return 1;
    }

    if (field_all_bodies(eph, &field) != CORE_OK) {
        return 1;
    }

    /* The catalogue publishes the stability index, which is
     * (lambda + 1/lambda)/2 - so index 297 means a perturbation is multiplied
     * by 594 per revolution, not 297. Saying which one this is, because the
     * two are easy to confuse (stm.h). */
    printf("ex_trajectory: catalogue orbit %d, stability index %.6g\n",
           halo[ORBIT].index, halo[ORBIT].stability);

    int ok = lay_out() && shoot() && export_trajectory() && export_station();

    eph_free(eph);
    return ok ? 0 : 1;
}
