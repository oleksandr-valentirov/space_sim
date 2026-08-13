#include "prop.h"

#include "field.h"
#include "integrator.h"

#include <stdlib.h>

struct PropagatorCtx {
    FieldCtx   field;
    PropConfig cfg;
};

/* Where the samples go. Lives on prop_run's stack rather than in the context:
 * a run is the only thing that has an output buffer, and a context that
 * remembered one would be a context two threads could not share. */
typedef struct {
    State *out;
    size_t cap;
    size_t count;
} Sink;

static int sample(const State *s, void *ctx)
{
    Sink *sink = (Sink *)ctx;

    sink->out[sink->count] = *s;
    sink->count++;

    return sink->count >= sink->cap;
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

    p->cfg = *cfg;
    *out = p;
    return CORE_OK;
}

void prop_free(PropagatorCtx *p)
{
    free(p);
}

CoreResult prop_run(PropagatorCtx *p, const State *initial, double t_end,
                    State *out_states, size_t out_cap, size_t *out_count,
                    State *out_final, CoreStopReason *out_stop,
                    double *in_out_step)
{
    if (p == NULL || initial == NULL || out_count == NULL ||
        out_final == NULL || out_stop == NULL || in_out_step == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (out_states != NULL && out_cap == 0) {
        return CORE_ERR_INVALID_ARG;
    }
    if (*in_out_step < 0.0) {
        return CORE_ERR_INVALID_ARG;
    }

    *out_count = 0;

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

    Sink sink;
    sink.out = out_states;
    sink.cap = out_cap;
    sink.count = 0;

    /* Cleared before the run, read after it: the flag is sticky by design, so
     * a context reused for a second run would otherwise report the first
     * run's failure (core/field.h). */
    p->field.failed = 0;

    CoreResult res = dop853_integrate_obs(accel_field, &p->field, initial,
                                          t_end, &dcfg, &st,
                                          out_states != NULL ? sample : NULL,
                                          &sink, out_final);
    if (res != CORE_OK) {
        return res;
    }
    if (p->field.failed) {
        return CORE_ERR_INVALID_ARG;
    }

    *out_count = sink.count;
    *in_out_step = st.h;

    /* The integrator writes t_end into the final state verbatim when it gets
     * there, and the time it actually reached when the observer stopped it
     * first. So this comparison is exact, and it stays right in the corner
     * case where the buffer fills on the very last step: the run did reach
     * t_end, and saying BUFFER_FULL there would send the caller back for a
     * leg of zero length. */
    *out_stop = out_final->t == t_end ? CORE_STOP_T_END : CORE_STOP_BUFFER_FULL;

    return CORE_OK;
}
