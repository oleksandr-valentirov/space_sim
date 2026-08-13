/* Export: the uncertainty mechanic, run against a real halo mission
 * (ROADMAP, "Відкрите питання" after C6; PROJECT.md section 8).
 *
 * PROJECT.md section 8 bets the game's main novelty on this: the vessel's
 * state is an ESTIMATE, not a fact. An injection error or a lapse in
 * tracking grows into a probability cloud; a tracking pass shrinks it; the
 * player trades knowledge against time. Until now that bet was argued from
 * the design document alone. This is the first place it is run against a
 * real, unstable halo orbit and real numbers - not to build the final
 * navigation filter, but to see whether the growth is dramatic enough, on
 * the mission this project already has (orbit 1151, lambda 594 per
 * revolution), for the decision to matter.
 *
 * Two files:
 *
 *   uncertainty.csv     position/velocity sigma at every patch point, and
 *                       which ones follow a tracking pass
 *   stdout              PROJECT.md's own example, run for real: what it
 *                       costs to correct right after the last pass versus
 *                       what it would have cost one pass earlier
 *
 * The propagation itself is exact, not sketched: the same state transition
 * matrix differential correction uses (stm.h, core/uncertainty.h),
 * integrated through the real ten-body field (accel_field_var), along the
 * same multiple-shooting-corrected reference core/export/ex_trajectory.c
 * flies. A tracking pass is not modelled - core/uncertainty.h's
 * uncertainty_scale is one knob standing in for "some measurement
 * happened", not a link budget. See core/uncertainty.h and ROADMAP.md for
 * what that does and does not claim.
 *
 * The setup (catalogue orbit 1151, eight legs per revolution, the committed
 * asset) matches core/scenario/sc_trajectory.c and core/export/ex_trajectory.c
 * on purpose, and is duplicated here rather than shared: this is diagnostic
 * code, and a third near-copy of ninety lines is cheaper than the Makefile
 * surgery a shared translation unit under core/export/ would need. Keep the
 * three in sync if the reference mission ever changes.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "csv.h"
#include "field.h"
#include "frame.h"
#include "refdata.h"
#include "shooting.h"
#include "station.h"
#include "stm.h"
#include "uncertainty.h"

#include <string.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3
#define MOON  4

#define ORBIT 3
#define LEGS 8
#define PATCHES 57 /* seven revolutions plus the closing point */

/* Illustrative nav numbers, not derived from a real link budget or a real
 * injection dispersion - the point of this export is whether growth on
 * THIS orbit is dramatic enough to matter, not what a real mission's
 * numbers would be. */
#define INITIAL_POS_SIGMA_M   1.0e3  /* 1 km */
#define INITIAL_VEL_SIGMA_MPS 1.0e-2 /* 1 cm/s */
#define PASS_INTERVAL 4              /* legs between passes: half a revolution */
#define PASS_VARIANCE_SHRINK 0.1     /* a pass divides variance by 10, sigma by sqrt(10) */

/* Same horizon convention ex_trajectory.c's own station-keeping demo uses. */
#define DECISION_CONTROL_INTERVAL 4
#define DECISION_HORIZON 4

static RefHalo halo[16];
static size_t n_halo;
static double halo_mu;

static EphemerisCtx *eph;
static FieldCtx field;

static State patch[PATCHES];
static double times[PATCHES];
static double workspace[SHOOTING_WORKSPACE(PATCHES)];

static double length_scale, rate_scale;

typedef struct {
    int patch;
    double sigma_before, sigma_after;
} Pass;

#define MAX_PASSES (PATCHES / PASS_INTERVAL + 1)
static Pass passes[MAX_PASSES];
static int n_passes;

/* ---- reference trajectory: identical to ex_trajectory.c's lay_out/shoot ---- */

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
        fprintf(stderr, "ex_uncertainty: multiple shooting did not converge\n");
        return 0;
    }

    printf("  shooting: %d iterations, worst gap %.3g m, %.3g m/s\n",
           report.iterations, report.worst_position_gap,
           report.worst_velocity_gap);
    return 1;
}

/* ---- covariance growth along the reference, with periodic tracking passes --- */

static int export_uncertainty(void)
{
    Csv c;
    if (!csv_open(&c, "build/csv/uncertainty.csv",
                  "t,days,pos_sigma_m,vel_sigma_mps,just_had_pass")) {
        return 0;
    }

    double p[STM_SIZE];
    memset(p, 0, sizeof p);
    for (int i = 0; i < 3; i++) {
        p[i * 6 + i] = INITIAL_POS_SIGMA_M * INITIAL_POS_SIGMA_M;
    }
    for (int i = 3; i < 6; i++) {
        p[i * 6 + i] = INITIAL_VEL_SIGMA_MPS * INITIAL_VEL_SIGMA_MPS;
    }

    csv_row(&c, 5, times[0], times[0] / 86400.0, uncertainty_position_sigma(p),
            uncertainty_velocity_sigma(p), 0.0);

    n_passes = 0;

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-2; /* matches shoot()'s own tolerance on this reference */
    cfg.max_steps = 1000000;

    for (int i = 0; i < PATCHES - 1; i++) {
        State from = patch[i];
        from.t = times[i];

        Dop853State st;
        memset(&st, 0, sizeof st);
        State arrival;
        double phi[STM_SIZE];
        if (stm_integrate(accel_field_var, &field, &from, times[i + 1], &cfg,
                          &st, &arrival, phi) != CORE_OK) {
            fprintf(stderr, "ex_uncertainty: STM integration failed at leg %d\n",
                    i);
            return 0;
        }

        double next_p[STM_SIZE];
        uncertainty_propagate(phi, p, next_p);
        memcpy(p, next_p, sizeof p);

        int just_had_pass = 0;
        if ((i + 1) % PASS_INTERVAL == 0) {
            double before = uncertainty_position_sigma(p);
            uncertainty_scale(p, PASS_VARIANCE_SHRINK);
            double after = uncertainty_position_sigma(p);

            if (n_passes < MAX_PASSES) {
                passes[n_passes].patch = i + 1;
                passes[n_passes].sigma_before = before;
                passes[n_passes].sigma_after = after;
                n_passes++;
            }
            just_had_pass = 1;
        }

        csv_row(&c, 5, times[i + 1], times[i + 1] / 86400.0,
                uncertainty_position_sigma(p), uncertainty_velocity_sigma(p),
                (double)just_had_pass);
    }

    return csv_close(&c);
}

/* ---- PROJECT.md section 8's example, with real numbers: burn now or wait --- */

/* The FIRST pass that leaves a full revolution of mission to correct with -
 * deliberately not the last. uncertainty.csv shows why: this orbit's
 * instability (594 per revolution) outgrows PASS_VARIANCE_SHRINK within a
 * couple of revolutions, and by the last eligible pass sigma has reached
 * ~10^14 m, a number with no scale left to reason about. That is a genuine
 * finding, not a bug - see ROADMAP.md - but it makes a poor demonstration
 * of the actual decision. The first pass sits at ~8 km, the same order as
 * PROJECT.md section 8's own example (+-40 km), and is where the "burn now
 * or wait" question still has two numbers worth comparing. */
static const Pass *pick_decision_pass(void)
{
    for (int i = 0; i < n_passes; i++) {
        if (passes[i].patch <= PATCHES - 1 - LEGS) {
            return &passes[i];
        }
    }
    return NULL;
}

static int decide(void)
{
    const Pass *pass = pick_decision_pass();
    if (pass == NULL) {
        printf("ex_uncertainty: no pass with a full revolution left to "
              "correct after it\n");
        return 1;
    }
    int last_pass_patch = pass->patch;
    double sigma_before = pass->sigma_before;
    double sigma_after = pass->sigma_after;

    StationConfig scfg;
    memset(&scfg, 0, sizeof scfg);
    scfg.tol_m = 1e-2;
    scfg.target_tol_m = 1.0;
    scfg.control_interval = DECISION_CONTROL_INTERVAL;
    scfg.horizon = DECISION_HORIZON;
    scfg.max_iterations = 15;

    size_t n = (size_t)(PATCHES - last_pass_patch);
    const State *reference = &patch[last_pass_patch];
    const double *ref_times = &times[last_pass_patch];

    State start_before = reference[0];
    start_before.r.x += sigma_before;

    State start_after = reference[0];
    start_after.r.x += sigma_after;

    StationReport before, after;
    if (station_keep(accel_field_var, &field, reference, ref_times, n,
                     &start_before, &scfg, &before) != CORE_OK
        || station_keep(accel_field_var, &field, reference, ref_times, n,
                        &start_after, &scfg, &after) != CORE_OK) {
        fprintf(stderr, "ex_uncertainty: station-keeping failed in the decision "
                        "comparison\n");
        return 0;
    }

    printf("\nPROJECT.md section 8's question, with real numbers "
          "(patch %d, %.1f days into the mission):\n",
          last_pass_patch, times[last_pass_patch] / 86400.0);
    printf("  1-sigma position uncertainty right before the pass: %.1f m\n",
          sigma_before);
    printf("  1-sigma position uncertainty right after the pass:  %.1f m\n",
          sigma_after);
    printf("  correcting from the BEFORE uncertainty costs %.4f m/s/yr "
          "(%d manoeuvres, %s)\n",
          before.per_year, before.manoeuvres,
          before.completed ? "completed" : "did not complete");
    printf("  correcting from the AFTER uncertainty costs  %.4f m/s/yr "
          "(%d manoeuvres, %s)\n",
          after.per_year, after.manoeuvres,
          after.completed ? "completed" : "did not complete");

    return 1;
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", halo, 16,
                          &n_halo) != CORE_OK
        || refdata_load_scalar("data/jpl_halo/mu.txt", &halo_mu) != CORE_OK
        || n_halo <= ORBIT) {
        fprintf(stderr, "ex_uncertainty: cannot read data/jpl_halo/\n");
        return 1;
    }

    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "ex_uncertainty: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds "
                        "it\n");
        return 1;
    }

    if (field_all_bodies(eph, &field) != CORE_OK) {
        return 1;
    }

    printf("ex_uncertainty: catalogue orbit %d, stability index %.6g\n",
          halo[ORBIT].index, halo[ORBIT].stability);

    int ok = lay_out() && shoot() && export_uncertainty() && decide();

    eph_free(eph);
    return ok ? 0 : 1;
}
