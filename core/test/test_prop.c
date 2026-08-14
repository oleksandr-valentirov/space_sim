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

#include "eph_build.h"
#include "field.h"
#include "integrator.h"
#include "prop.h"
#include "stm.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define ASSET "data/fixture/earth_moon.eph"

/* The one asset this file cooks rather than reads, for the one check the
 * fixture cannot express: a body with no radius (ROADMAP K7c, section 16). */
#define PAIR_PATH "build/test_prop_pair.eph"

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

/* Height above the body's own surface, read the way a caller would read it:
 * from the asset's radius, not from a number copied into this file. This is
 * the oracle CORE_EVENT_ALTITUDE is checked against (ROADMAP K7c). */
static double altitude_at(const EphemerisCtx *eph, const State *s)
{
    State earth;
    if (eph_body_state(eph, EARTH, s->t, &earth) != CORE_OK) {
        return -1.0e30;
    }
    return vec3_distance(s->r, earth.r) - eph_body_radius(eph, EARTH);
}

/* Propagate until the one armed event fires. Returns 0 if the run ended any
 * other way, so a test that stops believing the event happened says so. */
static int fires(PropagatorCtx *p, const State *start, const CoreEvent *ev,
                 double t_end, State *out)
{
    size_t n = 0;
    CoreStopReason stop = CORE_STOP_T_END;
    int event = -1;
    double step = 0.0;

    if (prop_run(p, start, NULL, t_end, ev, 1, NULL, 0, &n, out, &stop, &event,
                 &step) != CORE_OK) {
        return 0;
    }
    return stop == CORE_STOP_EVENT && event == 0;
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
static State vessel_at(const EphemerisCtx *eph, double t, double speed_factor)
{
    State earth;
    State s = { { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, t };

    if (eph_body_state(eph, EARTH, t, &earth) != CORE_OK) {
        return s;
    }

    double speed = speed_factor * sqrt(eph_body_mu(eph, EARTH) / ORBIT_R);

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
        int event = 0;

        if (!carry_step) {
            step = 0.0;
        }

        if (prop_run(p, &s, NULL, t_end, NULL, 0, chunk, cap, &n, &final_state,
                     &stop, &event, &step) != CORE_OK) {
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
    State start = vessel_at(eph, t0, 1.0);

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
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(p, &start, NULL, t_end, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
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
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(p, &start, NULL, t_end, NULL, 0, samples_one, BIG_CAP, &n_one,
                       &final_state, &stop, &event, &step) == CORE_OK);
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
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &start, NULL, t_span_end + 10.0 * DAY, NULL, 0, NULL, 0, &n,
                       &final_state, &stop, &event, &step) == CORE_ERR_INVALID_ARG);

        /* And the context is not poisoned by it: the sticky flag is cleared
         * at the start of every run, so the next one still works. */
        CHECK(prop_run(q, &start, NULL, t0 + HOUR, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);

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
        int event = 0;
        double step = 0.0;

        /* A buffer with no room in it: an immediate stop with no progress,
         * which a caller stitching legs would spin on forever. */
        CHECK(prop_run(q, &start, NULL, t_end, NULL, 0, samples_two, 0, &n,
                       &final_state, &stop, &event, &step)
              == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, NULL, NULL, t_end, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, &start, NULL, t_end, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, NULL) == CORE_ERR_INVALID_ARG);

        /* Zero length is a legal request and does nothing. */
        CHECK(prop_run(q, &start, NULL, start.t, NULL, 0, samples_two, BIG_CAP, &n,
                       &final_state, &stop, &event, &step) == CORE_OK);
        CHECK(n == 0);
        CHECK(stop == CORE_STOP_T_END);
        CHECK(same_state(&final_state, &start));

        prop_free(q);
    }

    /* ---- Events (ROADMAP H2) -------------------------------------------- *
     *
     * On an eccentric orbit, because a circular one has no periapsis worth
     * finding: d . d' hovers at zero the whole way round, and every step
     * would look like a crossing of nothing. Apoapsis is exactly where the
     * vessel starts - the position offset is along x and the velocity has no
     * x component, so the radial rate is zero to the bit. That is the awkward
     * case and it is deliberate: an event system that fires on the state it
     * was handed reports an event that has not happened. */

    State ecc = vessel_at(eph, t0, 0.8);
    double t_far = t0 + 4.0 * DAY;

    /* 8. Periapsis: found, and it is a real minimum of the distance. */
    double t_peri = 0.0;
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent ev = { CORE_EVENT_PERIAPSIS, EARTH, 0.0 };
        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &ecc, NULL, t_far, &ev, 1, samples_one, BIG_CAP, &n,
                       &final_state, &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(event == 0);
        CHECK(n > 0);

        /* The polyline ends at the event, not a step past it. */
        CHECK(same_state(&samples_one[n - 1], &final_state));

        t_peri = final_state.t;

        /* The oracle is the shape of the trajectory itself, not a number
         * copied from elsewhere: a minute either side of a minimum is
         * farther away. Nothing here knows what the answer should be. */
        State earth_at, before, after;
        CHECK(eph_body_state(eph, EARTH, final_state.t, &earth_at) == CORE_OK);
        double r_event = vec3_distance(final_state.r, earth_at.r);

        double probe_step = 0.0;
        size_t probe_n = 0;
        CoreStopReason probe_stop;
        int probe_event = 0;

        CHECK(prop_run(q, &ecc, NULL, final_state.t - 60.0, NULL, 0, NULL, 0,
                       &probe_n, &before, &probe_stop, &probe_event,
                       &probe_step) == CORE_OK);
        probe_step = 0.0;
        CHECK(prop_run(q, &final_state, NULL, final_state.t + 60.0, NULL, 0, NULL, 0,
                       &probe_n, &after, &probe_stop, &probe_event,
                       &probe_step) == CORE_OK);

        State earth_before, earth_after;
        CHECK(eph_body_state(eph, EARTH, before.t, &earth_before) == CORE_OK);
        CHECK(eph_body_state(eph, EARTH, after.t, &earth_after) == CORE_OK);

        double r_before = vec3_distance(before.r, earth_before.r);
        double r_after = vec3_distance(after.r, earth_after.r);

        CHECK(r_event < r_before);
        CHECK(r_event < r_after);

        printf("  periapsis at t0+%.3f h, r = %.3f km (%.3f / %.3f a minute "
               "either side)\n",
               (t_peri - t0) / HOUR, r_event / 1000.0, r_before / 1000.0,
               r_after / 1000.0);

        prop_free(q);
    }

    /* 9. The event time does not depend on where the steps fell.
     *
     *    This is the whole reason the run stops at the event instead of at the
     *    end of the step that crossed it (PROJECT.md section 4). Two runs with
     *    different step ceilings take different steps over the same
     *    trajectory; if the event time followed the steps, they would disagree
     *    by a step, which is thousands of seconds. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX / 4.0);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent ev = { CORE_EVENT_PERIAPSIS, EARTH, 0.0 };
        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &ecc, NULL, t_far, &ev, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);

        /* A step here is hundreds to thousands of seconds. The bound is
         * microseconds, and what is measured is smaller still. */
        double shift = final_state.t - t_peri;
        CHECK(shift < 1e-3 && shift > -1e-3);
        printf("  quarter the step ceiling moves the periapsis by %.3g s\n",
               shift);

        prop_free(q);
    }

    /* 10. Apoapsis, and the zero the run starts on.
     *
     *     The vessel begins exactly at apoapsis. Firing there would be the
     *     easy mistake, and it would be a bad one: a caller stopping at every
     *     apoapsis would stop forever without moving. The next apoapsis is one
     *     period later, and that is what must come back. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent ev = { CORE_EVENT_APOAPSIS, EARTH, 0.0 };
        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &ecc, NULL, t_far, &ev, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(final_state.t > t_peri);

        /* And from that apoapsis, the next one is a period further on rather
         * than immediately: the same zero, now reached rather than started
         * from. */
        State second;
        step = 0.0;
        CHECK(prop_run(q, &final_state, NULL, t_far, &ev, 1, NULL, 0, &n, &second,
                       &stop, &event, &step) == CORE_OK);

        double period = second.t - final_state.t;
        printf("  apoapsis at t0+%.3f h, the next one %.3f h later\n",
               (final_state.t - t0) / HOUR, period / HOUR);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(period > HOUR);

        /* Two apoapses either side of a periapsis, so a period is twice the
         * gap to it - a consistency check between two different events on the
         * same orbit, neither of which is told the answer. */
        double half = t_peri - t0;
        CHECK(period > 1.9 * half && period < 2.1 * half);

        prop_free(q);
    }

    /* 11. Distance, both ways across the same sphere, and the buffer losing
     *     to the event. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        double radius = 30000.0e3; /* between periapsis and apoapsis */
        CoreEvent ev = { CORE_EVENT_DISTANCE, EARTH, radius };

        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        /* A buffer of four, which would stop the run long before the event if
         * the event did not come first. */
        CHECK(prop_run(q, &ecc, NULL, t_far, &ev, 1, samples_two, SMALL_CAP, &n,
                       &final_state, &stop, &event, &step) == CORE_OK);

        State earth_at;
        CHECK(eph_body_state(eph, EARTH, final_state.t, &earth_at) == CORE_OK);
        double r_in = vec3_distance(final_state.r, earth_at.r);

        if (stop == CORE_STOP_EVENT) {
            CHECK(n > 0 && n <= SMALL_CAP);
            CHECK(same_state(&samples_two[n - 1], &final_state));
        } else {
            /* The crossing is farther away than four steps: legs first, then
             * the event. Either way the run below starts from where this one
             * stopped, so the test says the same thing. */
            CHECK(stop == CORE_STOP_BUFFER_FULL);
            for (int leg = 0; leg < 1000 && stop != CORE_STOP_EVENT; leg++) {
                State s = final_state;
                CHECK(prop_run(q, &s, NULL, t_far, &ev, 1, samples_two, SMALL_CAP,
                               &n, &final_state, &stop, &event, &step)
                      == CORE_OK);
            }
            CHECK(stop == CORE_STOP_EVENT);
            CHECK(eph_body_state(eph, EARTH, final_state.t, &earth_at)
                  == CORE_OK);
            r_in = vec3_distance(final_state.r, earth_at.r);
        }

        /* The root finder's own accuracy, measured rather than assumed: the
         * event state must be ON the sphere it was asked about. */
        double miss_in = r_in - radius;
        CHECK(miss_in < 1e-3 && miss_in > -1e-3);

        /* Outbound again, from just after the crossing. */
        State onward;
        step = 0.0;
        CHECK(prop_run(q, &final_state, NULL, t_far, &ev, 1, NULL, 0, &n, &onward,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(eph_body_state(eph, EARTH, onward.t, &earth_at) == CORE_OK);
        double miss_out = vec3_distance(onward.r, earth_at.r) - radius;
        CHECK(miss_out < 1e-3 && miss_out > -1e-3);

        printf("  distance %.0f km crossed inbound (miss %.3g m) and outbound "
               "(miss %.3g m), %.3f h apart\n",
               radius / 1000.0, miss_in, miss_out,
               (onward.t - final_state.t) / HOUR);

        prop_free(q);
    }

    /* 12. Two events armed at once, and the arithmetic that decides which
     *     one happened. The periapsis is inside the sphere, so the sphere is
     *     crossed first and must be the one reported. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent evs[2] = {
            { CORE_EVENT_PERIAPSIS, EARTH, 0.0 },
            { CORE_EVENT_DISTANCE, EARTH, 30000.0e3 },
        };

        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &ecc, NULL, t_far, evs, 2, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(event == 1);
        CHECK(final_state.t < t_peri);

        /* Carry on, and now the periapsis is the next thing to happen. */
        step = 0.0;
        State next;
        CHECK(prop_run(q, &final_state, NULL, t_far, evs, 2, NULL, 0, &n, &next,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(event == 0);

        double shift = next.t - t_peri;
        printf("  two events armed: sphere first, then periapsis %.3g s from "
               "where it was found alone\n", shift);
        CHECK(shift < 1e-3 && shift > -1e-3);

        prop_free(q);
    }

    /* 13. An event that never happens is not an event: the run reaches t_end
     *     and says so. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent ev = { CORE_EVENT_DISTANCE, EARTH, 1.0e12 };
        State final_state;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step = 0.0;

        CHECK(prop_run(q, &ecc, NULL, t0 + HOUR, &ev, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_T_END);
        CHECK(event == -1);

        /* Nonsense arguments are refused rather than quietly ignored. */
        CoreEvent bad = { CORE_EVENT_DISTANCE, EARTH, -1.0 };
        CHECK(prop_run(q, &ecc, NULL, t_far, &bad, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CoreEvent nobody = { CORE_EVENT_PERIAPSIS, 999, 0.0 };
        CHECK(prop_run(q, &ecc, NULL, t_far, &nobody, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, &ecc, NULL, t_far, &ev, PROP_MAX_EVENTS + 1, NULL, 0, &n,
                       &final_state, &stop, &event, &step)
              == CORE_ERR_INVALID_ARG);

        prop_free(q);
    }

    /* ---- Altitude (ROADMAP K7c) ----------------------------------------- *
     *
     * The same crossing counted from the surface instead of the centre. Three
     * separate things are being asked here, and only the first is about the
     * arithmetic:
     *
     *   14. it lands where it says it lands, and a band seam of the
     *       atmosphere table does not disturb it;
     *   15. it is the distance event with the asset's radius subtracted -
     *       not a second root finder that happens to agree;
     *   16. it refuses a body whose size the asset does not state, rather
     *       than measuring from a radius of zero.
     *
     * The orbit is new: everything above flies at geostationary radius, and
     * an altitude event needs a trajectory that reaches the air. Apoapsis
     * stays where it was, periapsis goes to 120 km. The speed factor is
     * derived from the asset's own radius rather than typed in, so recooking
     * the fixture cannot silently move this geometry. */

    double r_earth = eph_body_radius(eph, EARTH);
    CHECK(r_earth > 6.0e6 && r_earth < 6.5e6);

    double r_peri = r_earth + 120.0e3;
    State dive = vessel_at(eph, t0, sqrt(2.0 * r_peri / (ORBIT_R + r_peri)));

    /* 200 km is a base of the USSA-76 table (core/atmosphere.c), i.e. exactly
     * where the density is discontinuous; 220 km is inside a band. K7a was
     * caught out by probing on a base - finite differences measured the step
     * and not the slope - so the same trap is checked for here rather than
     * argued away. */
    const double H_SEAM = 200.0e3;
    const double H_MID = 220.0e3;

    /* 14. Found, and on the altitude asked for. */
    double t_alt_seam = 0.0;
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent seam = { CORE_EVENT_ALTITUDE, EARTH, H_SEAM };
        CoreEvent mid = { CORE_EVENT_ALTITUDE, EARTH, H_MID };

        State at_seam, at_mid;
        CHECK(fires(q, &dive, &seam, t_far, &at_seam));
        CHECK(fires(q, &dive, &mid, t_far, &at_mid));

        double miss_seam = altitude_at(eph, &at_seam) - H_SEAM;
        double miss_mid = altitude_at(eph, &at_mid) - H_MID;

        printf("  altitude on a band base missed by %.3g m, mid-band by "
               "%.3g m\n", miss_seam, miss_mid);
        CHECK(miss_seam < 1e-3 && miss_seam > -1e-3);
        CHECK(miss_mid < 1e-3 && miss_mid > -1e-3);

        /* The higher one is crossed first, which is the sanity check that the
         * vessel is descending and not being found on the way back up. */
        CHECK(at_mid.t < at_seam.t);

        t_alt_seam = at_seam.t;

        /* Zero is the surface and is allowed. This orbit does not reach it,
         * so the run ends at t_end - the point being that arming it is not
         * refused. */
        CoreEvent surface = { CORE_EVENT_ALTITUDE, EARTH, 0.0 };
        State ignored;
        size_t n = 0;
        CoreStopReason stop = CORE_STOP_EVENT;
        int event = 0;
        double step = 0.0;
        CHECK(prop_run(q, &dive, NULL, t0 + HOUR, &surface, 1, NULL, 0, &n,
                       &ignored, &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_T_END);

        /* Below the surface is a caller with a sign error, not a place. */
        CoreEvent below = { CORE_EVENT_ALTITUDE, EARTH, -1.0 };
        CHECK(prop_run(q, &dive, NULL, t_far, &below, 1, NULL, 0, &n, &ignored,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);

        prop_free(q);
    }

    /* 15. The same event as a distance of radius + altitude. Written as one
     *     function in core/prop.c precisely so this holds; the check is here
     *     because "one function" is an implementation detail and this is the
     *     promise made to callers. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        CoreEvent dist = { CORE_EVENT_DISTANCE, EARTH, r_earth + H_SEAM };
        State at_dist;
        CHECK(fires(q, &dive, &dist, t_far, &at_dist));

        /* Measured: the two agree to the bit, and the bound below is looser
         * than that on purpose. At the root the agreement is forced - both
         * forms subtract numbers within a factor of two of each other, where
         * floating-point subtraction is exact - but the Newton path leading
         * there passes through distances four times the radius, where
         * (d - R) - h and d - (R + h) may part by an ULP. So bit equality is
         * what happens, not what is promised, and pinning it here would make
         * a future change to the root finder look like a broken contract. */
        double shift = at_dist.t - t_alt_seam;
        printf("  altitude %.0f km and distance %.0f km differ by %.3g s\n",
               H_SEAM / 1000.0, (r_earth + H_SEAM) / 1000.0, shift);
        CHECK(shift < 1e-6 && shift > -1e-6);

        prop_free(q);
    }

    /* 16. A body whose size the asset does not state.
     *
     * Not reachable through the shipped fixture - every body in it is cited
     * with a radius (core/cook/cook_fixture.c) - so this cooks a two-body
     * asset of its own, identical in every respect except that one of the two
     * has no radius. One field of difference is what makes the refusal a
     * statement about the radius and not about the body.
     *
     * The system is invented rather than read from data/horizons: nothing
     * here depends on it being a real pair, and a synthetic one cannot drift
     * out of step with the reference files. */
    {
        NBodySystem sys;
        memset(&sys, 0, sizeof sys);
        sys.n = 2;
        sys.mu[0] = 3.986004418e14;
        sys.mu[1] = 4.9028e12;

        State init[NBODY_MAX];
        memset(init, 0, sizeof init);
        init[1].r.x = 3.844e8;
        init[1].v.y = 1022.0;

        static const EphBodyInfo pair[] = {
            { "sized", 6.371e6, 0.0, NULL },
            { "unsized", 0.0, 0.0, NULL },
        };

        EphBuildConfig bcfg;
        memset(&bcfg, 0, sizeof bcfg);
        bcfg.t_begin = 0.0;
        bcfg.t_end = 8.0 * DAY;
        bcfg.interval_seconds = 8.0 * DAY;
        bcfg.degree = 14;
        bcfg.orient_degree = 0;
        bcfg.tol_m = 1.0;

        EphBuildReport rep;
        memset(&rep, 0, sizeof rep);
        CHECK(eph_build(&sys, init, pair, &bcfg, PAIR_PATH, &rep) == CORE_OK);

        EphemerisCtx *two = NULL;
        CHECK(eph_load(PAIR_PATH, &two) == CORE_OK);
        if (two == NULL) {
            return EXIT_FAILURE;
        }
        CHECK(eph_body_radius(two, 0) == 6.371e6);
        CHECK(eph_body_radius(two, 1) == 0.0);

        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(two, &c, &q) == CORE_OK);

        State body;
        CHECK(eph_body_state(two, 0, 0.0, &body) == CORE_OK);
        State s = body;
        s.r.x += 6.771e6;
        s.v.y += 7672.0;

        State ignored;
        size_t n = 0;
        CoreStopReason stop = CORE_STOP_T_END;
        int event = 0;
        double step = 0.0;

        CoreEvent sized = { CORE_EVENT_ALTITUDE, 0, 100.0e3 };
        CoreEvent unsized = { CORE_EVENT_ALTITUDE, 1, 100.0e3 };
        CoreEvent by_distance = { CORE_EVENT_DISTANCE, 1, 100.0e3 };

        step = 0.0;
        CHECK(prop_run(q, &s, NULL, 60.0, &sized, 1, NULL, 0, &n, &ignored,
                       &stop, &event, &step) == CORE_OK);
        step = 0.0;
        CHECK(prop_run(q, &s, NULL, 60.0, &unsized, 1, NULL, 0, &n, &ignored,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        /* The same body, by distance, is fine: it is the altitude that has no
         * meaning here, not the body. */
        step = 0.0;
        CHECK(prop_run(q, &s, NULL, 60.0, &by_distance, 1, NULL, 0, &n, &ignored,
                       &stop, &event, &step) == CORE_OK);

        prop_free(q);
        eph_free(two);
    }

    /* prop_run_stm (ROADMAP K8), and the promise that makes it usable.
     *
     * The matrix is only worth having if it belongs to the trajectory the
     * vessel actually flies. That holds by construction rather than by
     * tolerance - core/dop853.c reads block 0 alone when it decides a step,
     * so the six variational blocks travel the same step sequence without
     * influencing it - and this is where that gets measured instead of
     * believed. */
    {
        PropConfig cfg_stm = config(H_MAX);
        PropagatorCtx *q = NULL;
        CHECK(prop_create(eph, &cfg_stm, &q) == CORE_OK);

        State start = vessel_at(eph, t0, 1.0);
        double t_stop = t0 + 0.5 * DAY;

        State plain_final;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double plain_step = 0.0;
        CHECK(prop_run(q, &start, NULL, t_stop, NULL, 0, NULL, 0, &n, &plain_final,
                       &stop, &event, &plain_step) == CORE_OK);
        CHECK(stop == CORE_STOP_T_END);

        State stm_final;
        double phi[STM_SIZE];
        double stm_step = 0.0;
        CHECK(prop_run_stm(q, &start, NULL, t_stop, &stm_final, phi, &stm_step)
              == CORE_OK);

        /* Bit-identical, position, velocity and the step left behind. The
         * step matters as much as the state: it is what the next call
         * continues with, and a run that agreed on the state while leaving
         * a different step would diverge from the following leg onwards. */
        CHECK_BITS_EQ(stm_final.r.x, plain_final.r.x);
        CHECK_BITS_EQ(stm_final.r.y, plain_final.r.y);
        CHECK_BITS_EQ(stm_final.r.z, plain_final.r.z);
        CHECK_BITS_EQ(stm_final.v.x, plain_final.v.x);
        CHECK_BITS_EQ(stm_final.v.y, plain_final.v.y);
        CHECK_BITS_EQ(stm_final.v.z, plain_final.v.z);
        CHECK_BITS_EQ(stm_step, plain_step);

        /* And the matrix is the real derivative of that trajectory, by
         * central differences through prop_run itself - so this compares
         * the STM against the propagator a caller would actually use, not
         * against a second copy of the same integration. */
        const double eps_r = 1.0e3;
        const double eps_v = 1.0e-3;

        double worst = 0.0, biggest = 0.0;

        for (int j = 0; j < 6; j++) {
            double eps = j < 3 ? eps_r : eps_v;

            State plus = start, minus = start;
            double *pp[6] = { &plus.r.x, &plus.r.y, &plus.r.z,
                              &plus.v.x, &plus.v.y, &plus.v.z };
            double *pm[6] = { &minus.r.x, &minus.r.y, &minus.r.z,
                              &minus.v.x, &minus.v.y, &minus.v.z };
            *pp[j] += eps;
            *pm[j] -= eps;

            State end_plus, end_minus;
            double h = 0.0;
            CHECK(prop_run(q, &plus, NULL, t_stop, NULL, 0, NULL, 0, &n, &end_plus,
                           &stop, &event, &h) == CORE_OK);
            h = 0.0;
            CHECK(prop_run(q, &minus, NULL, t_stop, NULL, 0, NULL, 0, &n, &end_minus,
                           &stop, &event, &h) == CORE_OK);

            double a[6] = { end_plus.r.x, end_plus.r.y, end_plus.r.z,
                            end_plus.v.x, end_plus.v.y, end_plus.v.z };
            double b[6] = { end_minus.r.x, end_minus.r.y, end_minus.r.z,
                            end_minus.v.x, end_minus.v.y, end_minus.v.z };

            for (int i = 0; i < 6; i++) {
                double numeric = (a[i] - b[i]) / (2.0 * eps);
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
        CHECK(worst < 1e-5 * biggest);
        printf("  stm matches finite differences to %.2g of %.4g\n",
               worst, biggest);

        /* Arguments checked, including the one that is easy to forget. */
        CHECK(prop_run_stm(NULL, &start, NULL, t_stop, &stm_final, phi, &stm_step)
              == CORE_ERR_INVALID_ARG);
        CHECK(prop_run_stm(q, &start, NULL, t_stop, &stm_final, NULL, &stm_step)
              == CORE_ERR_INVALID_ARG);
        CHECK(prop_run_stm(q, &start, NULL, t_stop, &stm_final, phi, NULL)
              == CORE_ERR_INVALID_ARG);

        /* Past the end of the EPHEMERIS - t_span_end, not the run's own
         * t_end, which is a day short of it and perfectly legal. Getting
         * that wrong is how the first version of this check passed while
         * asserting nothing.
         *
         * The field returns zero acceleration out there and sets its sticky
         * flag; without the check in prop_run_stm the caller would get
         * CORE_OK, a plausible trajectory of a vessel that felt no gravity,
         * and a matrix describing it. */
        double far_step = 0.0;
        CHECK(prop_run_stm(q, &start, NULL, t_span_end + DAY, &stm_final, phi,
                           &far_step) == CORE_ERR_INVALID_ARG);

        prop_free(q);
    }

    /* ---- A vessel that feels sunlight (ROADMAP K6b) --------------------- */

    /* Everything above passes NULL for the vessel, which is what every
     * caller did before this step existed. The three things worth checking
     * here are that NULL still means exactly that, that a vessel with an
     * area flies a measurably different trajectory, and that the vessel does
     * not leak from one run into the next through the shared context. */
    {
        PropagatorCtx *q = NULL;
        PropConfig c = config(H_MAX);
        CHECK(prop_create(eph, &c, &q) == CORE_OK);

        VesselParams sail = { 1000.0, 20.0, 1.3, 0.0 };
        VesselParams none = { 1000.0, 0.0, 1.3, 0.0 };

        State final_none, final_zero, final_sail, final_again;
        size_t n = 0;
        CoreStopReason stop;
        int event = 0;
        double step;

        step = 0.0;
        CHECK(prop_run(q, &start, NULL, t_end, NULL, 0, NULL, 0, &n,
                       &final_none, &stop, &event, &step) == CORE_OK);

        /* A vessel with no area is the massless particle, to the bit. */
        step = 0.0;
        CHECK(prop_run(q, &start, &none, t_end, NULL, 0, NULL, 0, &n,
                       &final_zero, &stop, &event, &step) == CORE_OK);
        CHECK(same_state(&final_zero, &final_none));

        step = 0.0;
        CHECK(prop_run(q, &start, &sail, t_end, NULL, 0, NULL, 0, &n,
                       &final_sail, &stop, &event, &step) == CORE_OK);

        /* Printed rather than merely bounded, because the size of it is
         * the check. A constant 1.23e-7 m/s^2 for two days is a free-flight
         * displacement of a*t^2/2 = 1.8 km; an orbit turns most of that into
         * a shifted ellipse rather than a drift, and what is left is 533 m.
         * Between the two, which is where it should be. Millimetres would
         * mean the term is being scaled away somewhere; hundreds of
         * kilometres would mean it is not radiation pressure. */
        double moved = vec3_distance(final_sail.r, final_none.r);
        printf("  two days under SRP move the vessel %.4g m\n", moved);
        CHECK(moved > 10.0);
        CHECK(moved < 1900.0);

        /* And back to NULL: the context must not remember the sail. Without
         * the per-run set in prop_run this is the check that fails, and it
         * fails as one spacecraft's area pushing the next one - which is
         * exactly what keeping the vessel out of PropConfig was meant to
         * prevent. */
        step = 0.0;
        CHECK(prop_run(q, &start, NULL, t_end, NULL, 0, NULL, 0, &n,
                       &final_again, &stop, &event, &step) == CORE_OK);
        CHECK(same_state(&final_again, &final_none));

        /* The STM path carries it too, and carries it the same way: the
         * matrix must belong to the trajectory the vessel actually flies,
         * which is the whole content of K8c. */
        double phi[STM_SIZE];
        State stm_final;
        double stm_step = 0.0;
        CHECK(prop_run_stm(q, &start, &sail, t_end, &stm_final, phi, &stm_step)
              == CORE_OK);
        CHECK(same_state(&stm_final, &final_sail));

        prop_free(q);
    }

    /* prop_free(NULL) is allowed - Drop on the Rust side frees without
     * asking (ROADMAP H4), the same promise eph_free already makes. */
    prop_free(NULL);

    eph_free(eph);
    return TEST_RESULT();
}
