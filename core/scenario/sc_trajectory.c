/* Determinism scenario: multiple shooting and station-keeping.
 *
 * Kept apart from sc_ephemeris so the two can be bisected against each other,
 * which is exactly the procedure ROADMAP C5 prescribes when a platform
 * disagrees. sc_ephemeris covers reading the asset and integrating in it;
 * this one covers what is built on top - the linear algebra of multiple
 * shooting and the targeting of station-keeping.
 *
 * It is the most branch-heavy scenario there is. A Newton iteration count, a
 * pivot choice in a 6x6 elimination and a convergence test are all decisions
 * taken on floating-point comparisons, so a platform that differs in the last
 * bit does not return a slightly different answer here - it returns a
 * different number of iterations, and the hash says so at once.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cr3bp.h"
#include "field.h"
#include "frame.h"
#include "hash.h"
#include "shooting.h"
#include "station.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3
#define MOON  4

#define LEGS 8
#define PATCHES 57          /* seven revolutions plus the closing point */

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static State patch[PATCHES];
static double times[PATCHES];
static double workspace[SHOOTING_WORKSPACE(PATCHES)];

static void hash_state(CoreHash *h, const State *s)
{
    core_hash_f64(h, s->r.x);
    core_hash_f64(h, s->r.y);
    core_hash_f64(h, s->r.z);
    core_hash_f64(h, s->v.x);
    core_hash_f64(h, s->v.y);
    core_hash_f64(h, s->v.z);
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "sc_trajectory: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);

    /* Catalogue orbit 1151 and the mass ratio it was found for. Written out
     * rather than read from data/jpl_halo, so this scenario depends on one
     * file rather than two. */
    Cr3bpCtx cr3bp = { opaque(1.215058560962404e-02) };
    State halo = {
        { opaque(1.1693640722281695), opaque(0.0),
          opaque(-9.6760151777927794e-02) },
        { opaque(0.0), opaque(-1.9391736078339492e-01), opaque(0.0) },
        opaque(0.0),
    };
    double period = opaque(3.3336235592858992);

    /* One revolution of patch points, then repeated - the orbit is periodic,
     * and propagating it for seven revolutions would let its own instability
     * (594 per revolution) destroy the reference. */
    double step = period / (double)LEGS;

    Dop853Config cr3bp_cfg = { 0 };
    cr3bp_cfg.tol_m = opaque(1e-13);
    cr3bp_cfg.max_steps = 1000000;

    State cycle[LEGS];
    State q = halo;

    for (int i = 0; i < LEGS; i++) {
        cycle[i] = q;

        Dop853State st = { 0 };
        State next;
        if (dop853_integrate(accel_cr3bp, &cr3bp, &q, q.t + step, &cr3bp_cfg,
                             &st, &next) != CORE_OK) {
            return 1;
        }
        q = next;
        hash_state(&h, &q);
    }

    double t = 0.0;
    double sum_length = 0.0, sum_rate = 0.0;

    for (int i = 0; i < PATCHES; i++) {
        SynodicFrame frame;
        if (frame_synodic(eph, EARTH, MOON, t, &frame) != CORE_OK) {
            return 1;
        }

        State dimensionless = cycle[i % LEGS];
        dimensionless.t = 0.0;

        times[i] = t;
        frame_to_inertial(&frame, &dimensionless, &patch[i]);

        sum_length += frame.length;
        sum_rate += frame.rate;

        t += step / frame.rate;
    }

    double length_scale = sum_length / (double)PATCHES;
    double rate_scale = sum_rate / (double)PATCHES;
    core_hash_f64(&h, length_scale);
    core_hash_f64(&h, rate_scale);
    core_hash_f64(&h, times[PATCHES - 1]);

    FieldCtx field;
    if (field_all_bodies(eph, &field) != CORE_OK) {
        return 1;
    }

    /* The asset's own field, harmonics and all (ROADMAP K8b). Between K4b
     * and K8a this had to be cleared: accel_field_var refused to linearise
     * a harmonic field, the refusal arrived as zero acceleration, multiple
     * shooting converged beautifully on a straight line, and this scenario
     * hashed it - a determinism check reproducing identical nonsense on
     * every platform. The field.failed check at the end is what turned
     * that from a hope into a statement, and it stays. */

    ShootingConfig shoot = { 0 };
    shoot.tol_m = opaque(1e-2);
    shoot.continuity_m = opaque(1.0);
    shoot.length_scale = length_scale;
    shoot.speed_scale = length_scale * rate_scale;
    shoot.max_iterations = 30;

    ShootingReport shot;
    if (shoot_multiple(accel_field_var, &field, patch, times, PATCHES, &shoot,
                       workspace, sizeof workspace / sizeof workspace[0],
                       &shot) != CORE_OK) {
        return 1;
    }

    core_hash_f64(&h, (double)shot.iterations);
    core_hash_f64(&h, shot.worst_position_gap);
    core_hash_f64(&h, shot.worst_velocity_gap);
    core_hash_f64(&h, shot.worst_step_m);
    for (int i = 0; i < PATCHES; i++) {
        hash_state(&h, &patch[i]);
    }

    /* Station-keeping from an injection error, which exercises the 3x3 solve
     * and the targeting loop. */
    StationConfig keep = { 0 };
    keep.tol_m = opaque(1e-2);
    keep.target_tol_m = opaque(1.0);
    keep.control_interval = 4;
    keep.horizon = 4;
    keep.max_iterations = 15;

    State start = patch[0];
    start.r.x += opaque(1.0e3);

    StationReport report;
    if (station_keep(accel_field_var, &field, patch, times, PATCHES, &start,
                     &keep, &report) != CORE_OK) {
        return 1;
    }

    core_hash_f64(&h, report.total_dv);
    core_hash_f64(&h, report.largest_dv);
    core_hash_f64(&h, report.per_year);
    core_hash_f64(&h, (double)report.manoeuvres);
    core_hash_f64(&h, report.flown);
    core_hash_f64(&h, report.worst_offset_m);

    /* The field must never have failed. A sticky flag nobody reads is not a
     * safeguard, and this scenario is precisely where that mattered: the
     * K4b refusal above would otherwise have been hashed as a result. */
    if (field.failed) {
        return 1;
    }
    core_hash_f64(&h, (double)report.completed);

    eph_free(eph);

    printf("sc_trajectory %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
