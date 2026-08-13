/* Single-shooting velocity targeting via the state transition matrix
 * (ROADMAP C4, G3).
 *
 * The shared primitive behind station-keeping (core/station.c, ROADMAP C4)
 * and refining a Lambert initial guess against the full ephemeris (ROADMAP
 * G3): given a departure position and time held fixed, find the departure
 * velocity that arrives at a target position at a target time.
 *
 * Newton's method on the position-vs-departure-velocity block of the state
 * transition matrix (core/stm.h): three unknowns, three equations. Unlike
 * core/shooting.c's block-tridiagonal system, no scaling is needed - every
 * entry of this 3x3 block has the same unit, seconds (it is d(position) /
 * d(velocity)), so it solves cleanly by Gaussian elimination with partial
 * pivoting. A three by three solve, so no libm: this lives in the
 * deterministic zone alongside the rest of the state-transition-matrix
 * machinery, even though one of its two callers (Lambert refinement) is
 * itself outside the determinism boundary (PROJECT.md section 4) - nothing
 * about the targeting math cares which side of that line its caller is on. */

#ifndef CORE_TARGET_H
#define CORE_TARGET_H

#include "integrator.h"

typedef struct {
    int    iterations;
    double miss_m; /* |aim - achieved position| at the end */
} TargetReport;

/* Adjust state->v (state->r and state->t are read but never changed - they
 * were never unknowns) so that propagating under f from state->t to t_aim
 * lands within tol_m of aim.
 *
 * report may be NULL. On success state->v holds the corrected departure
 * velocity. Returns CORE_ERR_INVALID_ARG for a NULL pointer, t_aim <=
 * state->t, or tol_m <= 0. Returns CORE_ERR_TOLERANCE_NOT_MET if a
 * propagation fails, the 3x3 block is singular - state->r and aim collinear
 * through the attracting bodies, the same degeneracy lambert_solve rejects
 * up front for the two-body case, but a Newton iterate here can walk into
 * even from a seed that did not - or max_iterations is exhausted; state is
 * left holding the best iterate reached either way. */
CoreResult target_hit(BlockAccelFunc f, void *ctx,
                      const Dop853Config *integrator_cfg,
                      State *state, double t_aim, Vec3d aim,
                      double tol_m, int max_iterations,
                      TargetReport *report);

#endif /* CORE_TARGET_H */
