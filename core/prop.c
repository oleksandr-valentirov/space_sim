#include "prop.h"

#include "field.h"
#include "integrator.h"
#include "stm.h"

#include <stdlib.h>

struct PropagatorCtx {
    FieldCtx   field;
    PropConfig cfg;
};

/* Everything one run needs. Lives on prop_run's stack rather than in the
 * context: a run is the only thing that has an output buffer and an event
 * list, and a context that remembered them would be a context two threads
 * could not share. */
typedef struct {
    PropagatorCtx *p;

    State *out;
    size_t cap;
    size_t count;

    const CoreEvent *events;
    size_t           n_events;

    /* The state at the end of the previous accepted step, and the event
     * functions there. A crossing is a disagreement between these and the
     * step that just finished. */
    State  prev;
    double g_prev[PROP_MAX_EVENTS];

    /* Which events crossed during the step that stopped the run, and the
     * value each had at the low end of its bracket. More than one can cross
     * in one step, and then the earliest in time is the one that happened. */
    int    crossed[PROP_MAX_EVENTS];
    double crossed_g_lo[PROP_MAX_EVENTS];
    int    n_crossed;

    /* An event function needs the body's state, and the ephemeris can refuse.
     * An observer has nowhere to return a code to, so it does what
     * core/field.h does with the same problem. */
    int failed;
} Run;

/* The scalar whose zero is the event, and its time derivative where that is
 * cheap.
 *
 * For a distance event the pair is exact. For periapsis and apoapsis the
 * derivative leaves out the body's own acceleration, which the ephemeris does
 * not carry - and that is harmless here, because the derivative only steers
 * the Newton step. Where the root actually is stays a question for the
 * bracket, which is decided by the sign of g and nothing else. */
static int event_value(Run *run, const CoreEvent *e, const State *s,
                       double *g_out, double *gdot_out, double *scale_out)
{
    State body;
    if (eph_body_state(run->p->field.eph, e->body_id, s->t, &body) != CORE_OK) {
        run->failed = 1;
        *g_out = 0.0;
        if (gdot_out != NULL) {
            *gdot_out = 0.0;
        }
        if (scale_out != NULL) {
            *scale_out = 1.0;
        }
        return 0;
    }

    Vec3d d = vec3_sub(s->r, body.r);
    Vec3d dv = vec3_sub(s->v, body.v);

    /* Distance and altitude are one function with one subtraction between
     * them, and they are written as one on purpose: two copies would be two
     * root finders to keep in agreement. The radius comes from the asset, so
     * an altitude here is above the same sphere the atmosphere measures from
     * (core/field.h) rather than above a number this file chose. */
    if (e->kind == CORE_EVENT_DISTANCE || e->kind == CORE_EVENT_ALTITUDE) {
        double dist = vec3_norm(d);
        double surface = e->kind == CORE_EVENT_ALTITUDE
                             ? eph_body_radius(run->p->field.eph, e->body_id)
                             : 0.0;
        *g_out = dist - surface - e->param;
        if (gdot_out != NULL) {
            *gdot_out = dist > 0.0 ? vec3_dot(d, dv) / dist : 0.0;
        }
        if (scale_out != NULL) {
            /* The distance, not the altitude: a run stopped at 100 km would
             * otherwise be classified as "at the event" within a nanometre of
             * the surface and within ten metres at geostationary, i.e. by a
             * threshold that swings by seven orders with the altitude asked
             * for. The distance from the centre is the one scale that does
             * not depend on where the caller drew the line. */
            *scale_out = dist;
        }
        return 1;
    }

    /* Periapsis and apoapsis are the same crossing read in opposite
     * directions: d . d' is the radial rate times the distance, negative
     * while falling and positive while climbing. Using it rather than the
     * distance itself matters - a minimum of one function is a plain sign
     * change of the other, and a sign change is what a bracket can find. */
    *g_out = vec3_dot(d, dv);

    if (gdot_out != NULL) {
        Vec3d a;
        accel_field(s->t, s->r, s->v, &run->p->field, &a);
        *gdot_out = vec3_dot(dv, dv) + vec3_dot(d, a);
    }
    if (scale_out != NULL) {
        *scale_out = vec3_norm(d) * vec3_norm(dv);
    }

    return 1;
}

/* Is this state AT the event rather than approaching it?
 *
 * The question has to be asked, and it has to be asked in a way that does not
 * invent a tolerance out of nothing. A run that stops at an apoapsis hands
 * back a state where g is zero to within the root finder's reach - and its
 * sign there is whatever the last arithmetic happened to give. Resume with the
 * same event armed, and half the time the very first step reads that sign as
 * "about to cross" and reports the apoapsis the caller just stopped at, at a
 * time indistinguishable from now. A caller stepping from apoapsis to apoapsis
 * would then never move.
 *
 * The threshold is relative to the natural scale of g, and the margin it lives
 * in is enormous rather than delicate. For an apoapsis of the orbit in
 * core/test/test_prop.c the scale is |d||d'| ~ 1.2e11, so the test admits
 * |g| < 0.12 - while a state one microsecond before that apoapsis already has
 * g ~ 1e1 and one second before it, g ~ 1e7. Eight orders separate "at the
 * event" from "very nearly at the event", which is what makes this a
 * classification rather than a tuned constant. */
#define AT_EVENT_REL 1e-12

static int at_event(double g, double scale)
{
    double bound = AT_EVENT_REL * scale;
    return g <= bound && g >= -bound;
}

/* The side of the crossing an event leaves behind. Only the sign is ever read
 * - g_prev is a sign, not a measurement, everywhere except inside find_event,
 * which uses it for a sign too. */
static double past_side(CoreEventKind kind, double gdot)
{
    switch (kind) {
    case CORE_EVENT_PERIAPSIS:
        return 1.0;   /* climbing away */
    case CORE_EVENT_APOAPSIS:
        return -1.0;  /* falling back */
    case CORE_EVENT_DISTANCE:
    case CORE_EVENT_ALTITUDE:
        /* Whichever way it is being crossed right now. A tangency (gdot = 0)
         * is not a crossing at all, so nothing is snapped. */
        return gdot > 0.0 ? 1.0 : (gdot < 0.0 ? -1.0 : 0.0);
    }
    return 0.0;
}

/* Did g cross the way this kind of event cares about?
 *
 * Zero counts as "not yet" on the side it is leaving, which is what makes a
 * run that starts exactly on an event not fire on it immediately - the same
 * problem core/correct.c steps over with a guard, solved here by the
 * comparison instead. */
static int crossed(CoreEventKind kind, double g_prev, double g_now)
{
    switch (kind) {
    case CORE_EVENT_PERIAPSIS:
        return g_prev < 0.0 && g_now >= 0.0;
    case CORE_EVENT_APOAPSIS:
        return g_prev > 0.0 && g_now <= 0.0;
    case CORE_EVENT_DISTANCE:
    case CORE_EVENT_ALTITUDE:
        return (g_prev < 0.0) != (g_now < 0.0);
    }
    return 0;
}

static int observe(const State *s, void *ctx)
{
    Run *run = (Run *)ctx;

    if (run->out != NULL) {
        run->out[run->count] = *s;
        run->count++;
    }

    run->n_crossed = 0;
    for (size_t i = 0; i < run->n_events; i++) {
        double g;
        if (!event_value(run, &run->events[i], s, &g, NULL, NULL)) {
            return 1;
        }
        if (crossed(run->events[i].kind, run->g_prev[i], g)) {
            run->crossed_g_lo[run->n_crossed] = run->g_prev[i];
            run->crossed[run->n_crossed] = (int)i;
            run->n_crossed++;
        }
        run->g_prev[i] = g;
    }

    if (run->n_crossed > 0) {
        /* prev stays where it was: it is the low end of the bracket. */
        return 1;
    }

    run->prev = *s;

    return run->out != NULL && run->count >= run->cap;
}

/* The state at time t, integrated from s0 with the run's own tolerance.
 *
 * A fresh step state each call, deliberately: the search must not disturb the
 * step the run carries. Same shape as the crossing search in core/correct.c,
 * for the same reason - this is a short leg inside one accepted step, so the
 * first step is clamped to it anyway. */
static CoreResult state_at(PropagatorCtx *p, const State *s0, double t,
                           State *out)
{
    Dop853Config dcfg;
    dcfg.tol_m = p->cfg.tol_m;
    dcfg.h_init = 0.0;
    dcfg.h_min = 0.0;
    dcfg.h_max = p->cfg.h_max_s;
    dcfg.max_steps = p->cfg.max_steps;

    Dop853State st = { 0.0, 0, 0, 0 };
    return dop853_integrate(accel_field, &p->field, s0, t, &dcfg, &st, out);
}

#define ROOT_ITERATIONS 100

/* Where in [s_lo->t, t_hi] the event is.
 *
 * Newton on g(t), safeguarded by the bracket: a step that leaves it is
 * replaced by a bisection. The same construction as core/correct.c and
 * core/planning/lambert.c, and for the same reason in all three - Newton does
 * the work, the bracket makes it impossible for Newton to be wrong.
 *
 * There is no tolerance to choose. The loop runs until the bracket stops
 * shrinking, which is a property of the arithmetic and therefore the same
 * number on every platform. */
static CoreResult find_event(Run *run, const CoreEvent *e, const State *s_lo,
                             double g_lo, double t_hi, State *out)
{
    double t_lo = s_lo->t;
    int sign_lo = g_lo < 0.0;

    double t = 0.5 * (t_lo + t_hi);
    State s;
    if (state_at(run->p, s_lo, t, &s) != CORE_OK) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    for (int i = 0; i < ROOT_ITERATIONS; i++) {
        double g, gdot;
        if (!event_value(run, e, &s, &g, &gdot, NULL)) {
            return CORE_ERR_INVALID_ARG;
        }

        if ((g < 0.0) == sign_lo) {
            t_lo = t;
        } else {
            t_hi = t;
        }

        double t_next;
        if (gdot != 0.0) {
            t_next = t - g / gdot;
        } else {
            t_next = 0.5 * (t_lo + t_hi);
        }
        if (!(t_next > t_lo) || !(t_next < t_hi)) {
            t_next = 0.5 * (t_lo + t_hi);
        }

        if (t_next == t) {
            break;
        }
        t = t_next;

        if (state_at(run->p, s_lo, t, &s) != CORE_OK) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
    }

    *out = s;
    return CORE_OK;
}

CoreResult prop_create(const EphemerisCtx *eph, const PropConfig *cfg,
                       PropagatorCtx **out)
{
    if (eph == NULL || cfg == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    *out = NULL;

    /* An integrator that does not exist yet is an argument error, not a
     * silent fall back to the one that does: a caller asking for RKN and
     * getting DOP853 would be told nothing and measure the wrong thing. */
    if (cfg->integrator != CORE_INTEG_DOP853) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol_m > 0.0) || cfg->h_max_s < 0.0 || cfg->max_steps < 0) {
        return CORE_ERR_INVALID_ARG;
    }
    /* See PropConfig::density_scale: zero is refused rather than read as one,
     * so that a caller who never set the field hears about it here instead of
     * flying through an atmosphere scaled by whatever the stack held. */
    if (!(cfg->density_scale > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    PropagatorCtx *p = calloc(1, sizeof *p);
    if (p == NULL) {
        /* Same code eph_load uses when a calloc fails. Not a great name for
         * out of memory, but CoreResult has four values and inventing a fifth
         * for a case that has never happened would widen the boundary for
         * nothing. */
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    CoreResult res = field_all_bodies(eph, &p->field);
    if (res != CORE_OK) {
        free(p);
        return res;
    }
    field_set_density_scale(&p->field, cfg->density_scale);

    p->cfg = *cfg;
    *out = p;
    return CORE_OK;
}

void prop_free(PropagatorCtx *p)
{
    free(p);
}

CoreResult prop_run(PropagatorCtx *p, const State *initial,
                    const VesselParams *vessel, double t_end,
                    const CoreEvent *events, size_t n_events,
                    State *out_states, size_t out_cap, size_t *out_count,
                    State *out_final, CoreStopReason *out_stop, int *out_event,
                    double *in_out_step)
{
    if (p == NULL || initial == NULL || out_count == NULL ||
        out_final == NULL || out_stop == NULL || out_event == NULL ||
        in_out_step == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (out_states != NULL && out_cap == 0) {
        return CORE_ERR_INVALID_ARG;
    }
    if (n_events > PROP_MAX_EVENTS || (n_events > 0 && events == NULL)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (*in_out_step < 0.0) {
        return CORE_ERR_INVALID_ARG;
    }

    for (size_t i = 0; i < n_events; i++) {
        if (events[i].body_id < 0 ||
            events[i].body_id >= eph_body_count(p->field.eph)) {
            return CORE_ERR_INVALID_ARG;
        }
        if (events[i].kind == CORE_EVENT_DISTANCE && !(events[i].param > 0.0)) {
            return CORE_ERR_INVALID_ARG;
        }
        if (events[i].kind == CORE_EVENT_ALTITUDE) {
            /* Zero is allowed and means the surface - the event a lithobraking
             * vessel arrives at, and the one a lander wants. Below it is not:
             * a negative altitude is a sphere inside the body, which is a
             * caller having got a sign wrong rather than a place to stop. */
            if (!(events[i].param >= 0.0)) {
                return CORE_ERR_INVALID_ARG;
            }
            /* The asset does not say how big this body is, so there is no
             * surface to be above. Refused rather than measured from zero:
             * a radius of zero would quietly make this a distance event, and
             * the trajectory would stop nine thousand kilometres from where
             * the caller meant. */
            if (!(eph_body_radius(p->field.eph, events[i].body_id) > 0.0)) {
                return CORE_ERR_INVALID_ARG;
            }
        }
    }

    *out_count = 0;
    *out_event = -1;

    Dop853Config dcfg;
    dcfg.tol_m = p->cfg.tol_m;
    dcfg.h_init = 0.0;
    dcfg.h_min = 0.0;
    dcfg.h_max = p->cfg.h_max_s;
    dcfg.max_steps = p->cfg.max_steps;

    /* Only h travels between calls. The counters are per-run diagnostics, and
     * carrying them would make the hash of a stitched run differ from the hash
     * of the same trajectory run in one go, for a reason that has nothing to
     * do with the trajectory. */
    Dop853State st;
    st.h = *in_out_step;
    st.n_accepted = 0;
    st.n_rejected = 0;
    st.n_evals = 0;

    Run run;
    run.p = p;
    run.out = out_states;
    run.cap = out_cap;
    run.count = 0;
    run.events = events;
    run.n_events = n_events;
    run.prev = *initial;
    run.n_crossed = 0;
    run.failed = 0;

    /* Cleared before the run, read after it: the flag is sticky by design, so
     * a context reused for a second run would otherwise report the first
     * run's failure (core/field.h). */
    p->field.failed = 0;

    /* Set every run, including to nothing when vessel is NULL. Leaving the
     * previous run's vessel in place would make one spacecraft's area push
     * the next one - the exact hazard that keeping this out of PropConfig
     * was meant to avoid, reintroduced in the context instead. */
    field_set_vessel(&p->field, vessel);

    /* The baseline every crossing is measured against. Without it the first
     * step would have nothing to disagree with.
     *
     * And an event the run is starting AT is one that has already happened -
     * see at_event. Its baseline is snapped to the far side, so the next
     * report of it is the next time it comes round. */
    for (size_t i = 0; i < n_events; i++) {
        double g, gdot, scale;
        if (!event_value(&run, &events[i], initial, &g, &gdot, &scale)) {
            return CORE_ERR_INVALID_ARG;
        }
        if (at_event(g, scale)) {
            double past = past_side(events[i].kind, gdot);
            if (past != 0.0) {
                g = past;
            }
        }
        run.g_prev[i] = g;
    }

    int watching = out_states != NULL || n_events > 0;

    CoreResult res = dop853_integrate_obs(accel_field, &p->field, initial,
                                          t_end, &dcfg, &st,
                                          watching ? observe : NULL,
                                          &run, out_final);
    if (res != CORE_OK) {
        return res;
    }
    if (p->field.failed || run.failed) {
        return CORE_ERR_INVALID_ARG;
    }

    *out_count = run.count;
    *in_out_step = st.h;

    /* The integrator writes t_end into the final state verbatim when it gets
     * there, and the time it actually reached when the observer stopped it
     * first. So this comparison is exact, and it stays right in the corner
     * case where the buffer fills on the very last step: the run did reach
     * t_end, and saying BUFFER_FULL there would send the caller back for a
     * leg of zero length. */
    *out_stop = out_final->t == t_end ? CORE_STOP_T_END : CORE_STOP_BUFFER_FULL;

    if (run.n_crossed == 0) {
        return CORE_OK;
    }

    /* One step, more than one crossing: whichever is earliest is the one that
     * happened, and the others did not happen yet. Refining all of them costs
     * a few short integrations and removes the need to argue about which
     * event "should" win. */
    State best = { { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, 0.0 };
    int best_index = -1;

    for (int i = 0; i < run.n_crossed; i++) {
        State at_event;
        res = find_event(&run, &events[run.crossed[i]], &run.prev,
                         run.crossed_g_lo[i], out_final->t, &at_event);
        if (res != CORE_OK) {
            return res;
        }
        if (best_index < 0 || at_event.t < best.t) {
            best = at_event;
            best_index = run.crossed[i];
        }
    }

    if (run.failed || p->field.failed) {
        return CORE_ERR_INVALID_ARG;
    }

    /* The last sample is past the event, so it is not on the trajectory the
     * caller is being handed. Replace it, and the polyline ends where the run
     * ended. */
    if (run.count > 0) {
        run.out[run.count - 1] = best;
    }

    *out_count = run.count;
    *out_final = best;
    *out_stop = CORE_STOP_EVENT;
    *out_event = best_index;

    return CORE_OK;
}

CoreResult prop_run_stm(PropagatorCtx *p, const State *initial,
                        const VesselParams *vessel, double t_end,
                        State *out_final, double out_stm[STM_SIZE],
                        double *in_out_step)
{
    if (p == NULL || initial == NULL || out_final == NULL ||
        out_stm == NULL || in_out_step == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (*in_out_step < 0.0) {
        return CORE_ERR_INVALID_ARG;
    }

    /* Identical to prop_run's, field for field. Sharing the lines would mean
     * a helper that takes a context and returns a config, which is more
     * machinery than the four assignments are worth; what matters is that
     * the two never differ, and that is what the bit-equality test in
     * core/test/test_prop.c is for. */
    Dop853Config dcfg;
    dcfg.tol_m = p->cfg.tol_m;
    dcfg.h_init = 0.0;
    dcfg.h_min = 0.0;
    dcfg.h_max = p->cfg.h_max_s;
    dcfg.max_steps = p->cfg.max_steps;

    Dop853State st;
    st.h = *in_out_step;
    st.n_accepted = 0;
    st.n_rejected = 0;
    st.n_evals = 0;

    p->field.failed = 0;
    field_set_vessel(&p->field, vessel);

    CoreResult res = stm_integrate(accel_field_var, &p->field, initial, t_end,
                                   &dcfg, &st, out_final, out_stm);
    if (res != CORE_OK) {
        return res;
    }

    /* The same check prop_run makes, and for the same reason: the field
     * returns zero acceleration outside the ephemeris span, which would be a
     * plausible trajectory of a vessel that felt no gravity - and a matrix
     * describing it. */
    if (p->field.failed) {
        return CORE_ERR_INVALID_ARG;
    }

    *in_out_step = st.h;
    return CORE_OK;
}
