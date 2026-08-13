/* The propagator behind the boundary (ROADMAP H1).
 *
 * prop_run does no arithmetic of its own - it hands accel_field to
 * dop853_integrate, which two other tests already cover. So what is tested
 * here is not the physics but the four promises the boundary makes about it,
 * and each one is a bit comparison rather than a tolerance, because each is a
 * statement about sameness and not about accuracy:
 *
 *   1. propagating without sampling is the same run as the direct call;
 *   2. asking for samples does not change the numbers;
 *   3. a run cut into pieces by a full buffer is the same trajectory as one
 *      uninterrupted run - CLAUDE.md invariant 5, and the reason a flight
 *      planner may draw a line the vessel will actually fly;
 *   4. and the step size carried between calls is what makes (3) true, which
 *      is checked by throwing it away and watching the trajectory change.
 *
 * Uses the committed fixture rather than a freshly cooked asset, same reason
 * as core/test/test_target.c: this is the runtime path, and the fixture is
 * what ships.
 *
 * Run from the repository root. */

#include "field.h"
#include "integrator.h"
#include "prop.h"
#include "test.h"

#include <math.h>
#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3

#define DAY 86400.0
#define HOUR 3600.0

/* Geostationary radius, and an orbit tilted out of the reference plane so
 * that no component of the state sits at zero for the whole run - a zero
 * hides a difference in exactly the comparisons this file is made of. */
#define ORBIT_R 42164.0e3

#define TOL_M 1e-2
#define H_MAX 1800.0

#define SPAN (2.0 * DAY)

static const size_t SMALL_CAP = 4;
static const size_t BIG_CAP = 4096;

static State samples_one[4096];
static State samples_two[4096];

static PropConfig config(double h_max_s)
{
    PropConfig cfg;
    cfg.integrator = CORE_INTEG_DOP853;
    cfg.tol_m = TOL_M;
    cfg.h_max_s = h_max_s;
    cfg.max_steps = 0;
    return cfg;
}

static Dop853Config direct_config(double h_max_s)
{
    Dop853Config cfg = { 0 };
    cfg.tol_m = TOL_M;
    cfg.h_max = h_max_s;
    return cfg;
}

static int same_state(const State *a, const State *b)
{
    return a->r.x == b->r.x && a->r.y == b->r.y && a->r.z == b->r.z &&
           a->v.x == b->v.x && a->v.y == b->v.y && a->v.z == b->v.z &&
           a->t == b->t;
}

/* A circular orbit about the Earth, expressed in the barycentric frame the
 * asset uses: the Earth's own state plus a relative position and the speed
 * that closes a circle at that radius. sqrt is the only function used, so the
 * setup itself stays inside the rules the core is built under. */
static State vessel_at(const EphemerisCtx *eph, double t)
{
    State earth;
    State s = { { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, t };

    if (eph_body_state(eph, EARTH, t, &earth) != CORE_OK) {
        return s;
    }

    double speed = sqrt(eph_body_mu(eph, EARTH) / ORBIT_R);

    /* (0, 0.8, 0.6) is a unit vector exactly, in binary as well as in
     * decimal: the inclination is a real one and it costs no rounding. */
    s.r.x = earth.r.x + ORBIT_R;
    s.r.y = earth.r.y;
    s.r.z = earth.r.z;

    s.v.x = earth.v.x;
    s.v.y = earth.v.y + 0.8 * speed;
    s.v.z = earth.v.z + 0.6 * speed;

    return s;
}

/* One run, stopping as often as the buffer says, resuming from where it
 * stopped. Returns the number of legs, or -1 on failure. carry_step = 0
 * throws the integrator's step away between legs, which is the counter-test:
 * the promise is that carrying it matters. */
static int stitch(PropagatorCtx *p, const State *start, double t_end,
                  size_t cap, int carry_step, State *out_final,
                  size_t *out_total, double *out_step)
{
    State s = *start;
    double step = 0.0;
    size_t total = 0;

    for (int leg = 1; leg <= 100000; leg++) {
        State chunk[64];
        size_t n = 0;
        State final_state;
        CoreStopReason stop;

        if (!carry_step) {
            step = 0.0;
        }

        if (prop_run(p, &s, t_end, chunk, cap, &n, &final_state, &stop, &step)
            != CORE_OK) {
            return -1;
        }

        total += n;
        s = final_state;

        if (stop == CORE_STOP_T_END) {
            *out_final = final_state;
            *out_total = total;
            *out_step = step;
            return leg;
        }
    }

    return -1;
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    CHECK(eph_load(ASSET, &eph) == CORE_OK);
    if (eph == NULL) {
        fprintf(stderr, "test_prop: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return TEST_RESULT();
    }

    double t_begin = 0.0, t_span_end = 0.0;
    CHECK(eph_span(eph, &t_begin, &t_span_end) == CORE_OK);

    double t0 = t_begin + 1.0 * DAY;
    double t_end = t0 + SPAN;
    State start = vessel_at(eph, t0);

    /* ---- The direct run, which is the oracle for everything below ------- */

    FieldCtx field;
    CHECK(field_all_bodies(eph, &field) == CORE_OK);

    Dop853Config dcfg = direct_config(H_MAX);
    Dop853State dst = { 0.0, 0, 0, 0 };
    State direct;
    CHECK(dop853_integrate(accel_field, &field, &start, t_end, &dcfg, &dst,
                           &direct) == CORE_OK);
    CHECK(field.failed == 0);
    CHECK(dst.n_accepted > 10);

    PropagatorCtx *p = NULL;
    PropConfig cfg = config(H_MAX);
    CHECK(prop_create(eph, &cfg, &p) == CORE_OK);

    /* 1. No samples: the boundary must add nothing at all. */
    {
        State final_state;
        size_t n = 1;
        CoreStopReason stop;
        double step = 0.0;

        CHECK(prop_run(p, &start, t_end, NULL, 0, &n, &final_state, &stop,
                       &step) == CORE_OK);
        CHECK(same_state(&final_state, &direct));
        CHECK(n == 0);
        CHECK(stop == CORE_STOP_T_END);
        CHECK_BITS_EQ(step, dst.h);
    }

    /* 2. Sampling is free: the same numbers come out, and the samples are the
     *    accepted steps rather than some grid of their own. */
    size_t n_one = 0;
    {
        State final_state;
        CoreStopReason stop;
        double step = 0.0;

        CHECK(prop_run(p, &start, t_end, samples_one, BIG_CAP, &n_one,
                       &final_state, &stop, &step) == CORE_OK);
        CHECK(same_state(&final_state, &direct));
        CHECK(stop == CORE_STOP_T_END);
        CHECK(n_one == (size_t)dst.n_accepted);
        CHECK(n_one > 0 && n_one < BIG_CAP);

        /* The last sample is the end of the last step, so it is the final
         * state - not a copy of it made separately. */
        CHECK(same_state(&samples_one[n_one - 1], &final_state));

        /* And the first sample is a step ahead of the start, never the start
         * itself: stitched legs must not repeat a vertex. */
        CHECK(samples_one[0].t > start.t);
    }

    /* 3. The same trajectory, cut into pieces of four samples. */
    {
        State final_state;
        size_t total = 0;
        double step = 0.0;
        int legs = stitch(p, &start, t_end, SMALL_CAP, 1, &final_state, &total,
                          &step);

        CHECK(legs > 1);
        CHECK(same_state(&final_state, &direct));
        CHECK(total == n_one);

        /* Not only the same trajectory but the same place to carry on from:
         * the step the last leg leaves behind is the step the uninterrupted
         * run leaves behind. Without this the two would agree up to t_end and
         * then part company on whatever comes after it. */
        CHECK_BITS_EQ(step, dst.h);

        printf("  stitched %d legs of %zu samples, %zu samples total\n",
               legs, SMALL_CAP, total);
    }

    /* 4. The counter-test. Same stitching, but the integrator's step is
     *    thrown away at every leg boundary, which is exactly what "resume
     *    with a fresh step" means in a save file. The trajectory must come
     *    out different - if it did not, the step would not be worth saving
     *    and PROJECT.md section 4 would be carrying a rule for nothing. */
    {
        State final_state;
        size_t total = 0;
        double step = 0.0;
        int legs = stitch(p, &start, t_end, SMALL_CAP, 0, &final_state, &total,
                          &step);

        CHECK(legs > 1);
        CHECK(!same_state(&final_state, &direct));

        double dx = final_state.r.x - direct.r.x;
        double dy = final_state.r.y - direct.r.y;
        double dz = final_state.r.z - direct.r.z;
        printf("  step discarded: %zu samples (%zu with it), position moves "
               "%.3g m\n", total, n_one, sqrt(dx * dx + dy * dy + dz * dz));
    }

    prop_free(p);

    /* 5. The same stitching with no ceiling on the step, which is where the
     *    guarantee actually stops.
     *
     *    The ceiling the integrator picks for itself is the length of the leg
     *    it was given (core/dop853.c), so the last leg of a stitched run gets
     *    a ceiling of its own short span. Measured: the trajectory still comes
     *    out identical, and the step left behind does not - which is the worse
     *    half of the two, because it is not visible in the answer. A caller
     *    who then propagates past t_end continues with a step nobody chose.
     *
     *    Asserted as an inequality on purpose. If this ever starts matching,
     *    something changed about how a clamped step feeds the controller, and
     *    that is worth being told about rather than silently inheriting. */
    {
        PropagatorCtx *q = NULL;
        PropConfig loose = config(0.0);
        CHECK(prop_create(eph, &loose, &q) == CORE_OK);

        Dop853Config ucfg = direct_config(0.0);
        Dop853State ust = { 0.0, 0, 0, 0 };
        State unlimited;
        CHECK(dop853_integrate(accel_field, &field, &start, t_end, &ucfg, &ust,
                               &unlimited) == CORE_OK);

        State final_state;
        size_t total = 0;
        double step = 0.0;
        int legs = stitch(q, &start, t_end, SMALL_CAP, 1, &final_state, &total,
                          &step);
        CHECK(legs > 1);

        double dx = final_state.r.x - unlimited.r.x;
        double dy = final_state.r.y - unlimited.r.y;
        double dz = final_state.r.z - unlimited.r.z;
        CHECK(step != ust.h);

        printf("  h_max_s = 0: %zu samples (%ld accepted in one run), "
               "position differs by %.3g m, step %.17g vs %.17g\n",
               total, ust.n_accepted, sqrt(dx * dx + dy * dy + dz * dz),
               step, ust.h);

        prop_free(q);
    }

    /* 6. Off the end of the asset. The field goes quiet there rather than
     *    failing loudly on its own (core/field.h), so a propagator that did
     *    not check would return a vessel coasting in a straight line through
     *    a solar system that had stopped pulling on it. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        double step = 0.0;

        CHECK(prop_run(q, &start, t_span_end + 10.0 * DAY, NULL, 0, &n,
                       &final_state, &stop, &step) == CORE_ERR_INVALID_ARG);

        /* And the context is not poisoned by it: the sticky flag is cleared
         * at the start of every run, so the next one still works. */
        CHECK(prop_run(q, &start, t0 + HOUR, NULL, 0, &n, &final_state, &stop,
                       &step) == CORE_OK);

        prop_free(q);
    }

    /* 7. Arguments. Each of these is a mistake that would otherwise be
     *    diagnosed far from where it was made. */
    {
        PropagatorCtx *q = NULL;

        PropConfig rkn = config(H_MAX);
        rkn.integrator = CORE_INTEG_RKN;
        CHECK(prop_create(eph, &rkn, &q) == CORE_ERR_INVALID_ARG);
        CHECK(q == NULL);

        PropConfig no_tol = config(H_MAX);
        no_tol.tol_m = 0.0;
        CHECK(prop_create(eph, &no_tol, &q) == CORE_ERR_INVALID_ARG);

        PropConfig ok = config(H_MAX);
        CHECK(prop_create(eph, &ok, &q) == CORE_OK);

        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        double step = 0.0;

        /* A buffer with no room in it: an immediate stop with no progress,
         * which a caller stitching legs would spin on forever. */
        CHECK(prop_run(q, &start, t_end, samples_two, 0, &n, &final_state,
                       &stop, &step) == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, NULL, t_end, NULL, 0, &n, &final_state, &stop, &step)
              == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, &start, t_end, NULL, 0, &n, &final_state, &stop, NULL)
              == CORE_ERR_INVALID_ARG);

        /* Zero length is a legal request and does nothing. */
        CHECK(prop_run(q, &start, start.t, samples_two, BIG_CAP, &n,
                       &final_state, &stop, &step) == CORE_OK);
        CHECK(n == 0);
        CHECK(stop == CORE_STOP_T_END);
        CHECK(same_state(&final_state, &start));

        prop_free(q);
    }

    /* prop_free(NULL) is allowed - Drop on the Rust side frees without
     * asking (ROADMAP H4), the same promise eph_free already makes. */
    prop_free(NULL);

    eph_free(eph);
    return TEST_RESULT();
}
