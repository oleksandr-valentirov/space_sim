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

        if (prop_run(p, &s, t_end, NULL, 0, chunk, cap, &n, &final_state,
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

        CHECK(prop_run(p, &start, t_end, NULL, 0, NULL, 0, &n, &final_state,
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

        CHECK(prop_run(p, &start, t_end, NULL, 0, samples_one, BIG_CAP, &n_one,
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

        CHECK(prop_run(q, &start, t_span_end + 10.0 * DAY, NULL, 0, NULL, 0, &n,
                       &final_state, &stop, &event, &step) == CORE_ERR_INVALID_ARG);

        /* And the context is not poisoned by it: the sticky flag is cleared
         * at the start of every run, so the next one still works. */
        CHECK(prop_run(q, &start, t0 + HOUR, NULL, 0, NULL, 0, &n, &final_state,
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
        CHECK(prop_run(q, &start, t_end, NULL, 0, samples_two, 0, &n,
                       &final_state, &stop, &event, &step)
              == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, NULL, t_end, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, &start, t_end, NULL, 0, NULL, 0, &n, &final_state,
                       &stop, &event, NULL) == CORE_ERR_INVALID_ARG);

        /* Zero length is a legal request and does nothing. */
        CHECK(prop_run(q, &start, start.t, NULL, 0, samples_two, BIG_CAP, &n,
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

        CHECK(prop_run(q, &ecc, t_far, &ev, 1, samples_one, BIG_CAP, &n,
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

        CHECK(prop_run(q, &ecc, final_state.t - 60.0, NULL, 0, NULL, 0,
                       &probe_n, &before, &probe_stop, &probe_event,
                       &probe_step) == CORE_OK);
        probe_step = 0.0;
        CHECK(prop_run(q, &final_state, final_state.t + 60.0, NULL, 0, NULL, 0,
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

        CHECK(prop_run(q, &ecc, t_far, &ev, 1, NULL, 0, &n, &final_state,
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

        CHECK(prop_run(q, &ecc, t_far, &ev, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(final_state.t > t_peri);

        /* And from that apoapsis, the next one is a period further on rather
         * than immediately: the same zero, now reached rather than started
         * from. */
        State second;
        step = 0.0;
        CHECK(prop_run(q, &final_state, t_far, &ev, 1, NULL, 0, &n, &second,
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
        CHECK(prop_run(q, &ecc, t_far, &ev, 1, samples_two, SMALL_CAP, &n,
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
                CHECK(prop_run(q, &s, t_far, &ev, 1, samples_two, SMALL_CAP,
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
        CHECK(prop_run(q, &final_state, t_far, &ev, 1, NULL, 0, &n, &onward,
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

        CHECK(prop_run(q, &ecc, t_far, evs, 2, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_EVENT);
        CHECK(event == 1);
        CHECK(final_state.t < t_peri);

        /* Carry on, and now the periapsis is the next thing to happen. */
        step = 0.0;
        State next;
        CHECK(prop_run(q, &final_state, t_far, evs, 2, NULL, 0, &n, &next,
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

        CHECK(prop_run(q, &ecc, t0 + HOUR, &ev, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_OK);
        CHECK(stop == CORE_STOP_T_END);
        CHECK(event == -1);

        /* Nonsense arguments are refused rather than quietly ignored. */
        CoreEvent bad = { CORE_EVENT_DISTANCE, EARTH, -1.0 };
        CHECK(prop_run(q, &ecc, t_far, &bad, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CoreEvent nobody = { CORE_EVENT_PERIAPSIS, 999, 0.0 };
        CHECK(prop_run(q, &ecc, t_far, &nobody, 1, NULL, 0, &n, &final_state,
                       &stop, &event, &step) == CORE_ERR_INVALID_ARG);
        CHECK(prop_run(q, &ecc, t_far, &ev, PROP_MAX_EVENTS + 1, NULL, 0, &n,
                       &final_state, &stop, &event, &step)
              == CORE_ERR_INVALID_ARG);

        prop_free(q);
    }

    /* prop_free(NULL) is allowed - Drop on the Rust side frees without
     * asking (ROADMAP H4), the same promise eph_free already makes. */
    prop_free(NULL);

    eph_free(eph);
    return TEST_RESULT();
}
