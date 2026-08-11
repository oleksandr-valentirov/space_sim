/* Integrators (ROADMAP B3, B4).
 *
 * RK4 is here as a stepping stone, not as the answer. The runtime integrator
 * is DOP853 (PROJECT.md section 4). RK4 exists because it is thirty lines,
 * which means it validates the force model, the state layout and the test
 * harness before there is any question of looking for a mistake inside an
 * adaptive step controller. Once DOP853 lands, RK4 stays as the independent
 * second implementation to cross-check it against. */

#ifndef CORE_INTEGRATOR_H
#define CORE_INTEGRATOR_H

#include "accel.h"
#include "core.h"

#include <stddef.h>

/* One fixed step of size h. in and out may not alias. */
CoreResult rk4_step(AccelFunc f, void *ctx, const State *in, double h,
                    State *out);

/* Fixed-step integration from in->t to t_end.
 *
 * h is a request, not a promise: the interval is divided into a whole number
 * of equal steps close to h, so the end time is reached exactly and every
 * step is the same size. A ragged final step would quietly contaminate any
 * measurement of the method's order, which is the main thing RK4 is here to
 * demonstrate.
 *
 * The direction of h is ignored; t_end decides whether time runs forwards or
 * backwards. Backwards integration is what the reversibility test needs. */
CoreResult rk4_integrate(AccelFunc f, void *ctx, const State *in,
                         double t_end, double h, State *out);

/* ---- DOP853: the runtime integrator ------------------------------------ */

typedef struct {
    /* Absolute position tolerance in metres. Not relative: a relative
     * tolerance behaves badly near a coordinate zero, which a barycentric
     * frame crosses constantly (PROJECT.md section 4).
     *
     * The velocity scale is derived as tol_m / |h|: the velocity error that
     * would accumulate to tol_m of position over one step. So there is one
     * number for the caller to choose, and it is in metres. */
    double tol_m;

    double h_init;    /* 0 -> chosen automatically */
    double h_min;     /* 0 -> no floor; a step below this is an error */
    double h_max;     /* 0 -> no ceiling */
    long   max_steps; /* 0 -> default */
} Dop853Config;

typedef struct {
    /* The step size to start the next integration with. This is why the type
     * exists: PROJECT.md section 4 requires the integrator's step to be part
     * of the save. An adaptive step sequence depends on its own history, so
     * resuming from a "fresh" step produces a different trajectory from the
     * one that was saved, and in an N-body system that difference grows. */
    double h;

    long n_accepted;
    long n_rejected;
    long n_evals;
} Dop853State;

/* Adaptive integration from in->t to t_end.
 *
 * io->h is read on entry (0 means "choose") and left holding the step the
 * next call should continue with. Pass the same Dop853State back to continue
 * a trajectory; zero it to start one.
 *
 * Direction comes from t_end, so backwards integration works. */
CoreResult dop853_integrate(AccelFunc f, void *ctx, const State *in,
                            double t_end, const Dop853Config *cfg,
                            Dop853State *io, State *out);

#endif /* CORE_INTEGRATOR_H */
