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

/* ---- Blocks: carrying companion trajectories through the same steps ------ */

/* Everything below exists to serve one requirement, and it is worth naming it
 * before the mechanism: the state transition matrix (PROJECT.md section 5,
 * prop_run_stm). Differential correction needs it, and so does the uncertainty
 * machinery in section 8.
 *
 * The variational equations that produce the STM are six extra
 * (position, velocity) pairs riding alongside the trajectory, each obeying
 * d(dr)/dt = dv exactly as the state does. So rather than a second integrator
 * for a 42-dimensional system, DOP853 here steps an array of blocks: block 0
 * is the trajectory, blocks 1..n-1 are whatever travels with it.
 *
 * They must share one step sequence. A separately integrated STM would be the
 * derivative of a slightly different trajectory than the one it is paired
 * with, and the two would disagree exactly where correction needs them to
 * agree - near the singular passages where the step is small.
 *
 * dop853_integrate is a one-block call into this, which keeps the invariant
 * from CLAUDE.md - one integrator, one tolerance - true by construction rather
 * than by discipline. */

#define DOP853_MAX_BLOCKS 7

/* Accelerations for every block at once. r, v and a_out are arrays of
 * n_blocks entries; block 0 is the reference trajectory, and the rest may
 * depend on it, which is exactly what a variational equation does. */
typedef void (*BlockAccelFunc)(double t, const Vec3d *r, const Vec3d *v,
                               int n_blocks, void *ctx, Vec3d *a_out);

/* Adaptive integration of n_blocks blocks from t0 to t_end, in place.
 *
 * Step size control reads block 0 only. That is a deliberate choice, not an
 * oversight: cfg->tol_m is a tolerance in metres on a trajectory, and the
 * variational blocks are dimensionless sensitivities with no such scale. What
 * keeps them honest is that their accuracy follows the trajectory's - and the
 * proof of it is the finite-difference test in core/test/test_stm.c, which is
 * the diagnostic ROADMAP C2b asks for. */
CoreResult dop853_integrate_blocks(BlockAccelFunc f, void *ctx, int n_blocks,
                                   double t0, double t_end,
                                   Vec3d *r, Vec3d *v,
                                   const Dop853Config *cfg, Dop853State *io);

#endif /* CORE_INTEGRATOR_H */
