/* Multiple shooting (ROADMAP C4).
 *
 * A trajectory in an unstable region cannot be found by propagating from one
 * end. Near L2 a perturbation is multiplied by 150 per revolution
 * (core/test/test_stability.c), so a correction applied at the start has to be
 * accurate to a part in 10^15 to survive ten revolutions - which is to say it
 * cannot be done. The instability is physical and no integrator tolerance
 * removes it.
 *
 * Multiple shooting sidesteps it by never propagating far. The trajectory is
 * carried as a sequence of states at fixed times, each propagated only as far
 * as the next, and the unknowns are all of them at once. No single leg is long
 * enough for the instability to matter, and the linear system that ties them
 * together is solved globally, so the conditioning of the whole is the
 * conditioning of one leg rather than of the span.
 *
 * ---
 *
 * There are more unknowns here than equations - 6n states against 6(n-1)
 * continuity conditions - and that is deliberate. Rather than pinning six
 * numbers to make the system square, which forces a choice of which end to
 * believe, the extra freedom is spent on staying near the initial guess: of
 * all continuous trajectories, the one closest to the CR3BP orbit that was
 * carried over.
 *
 * That is not the same as taking the smallest STEP at each iteration, and the
 * difference is the whole result rather than a refinement. Minimising the step
 * converges perfectly well - measured continuity of 1e-4 m over 350 days - to
 * a trajectory twelve length units away from L2, because the individually
 * small steps all point the same way and accumulate. Minimising the distance
 * to the guess instead pulls the iterate back at every step, and the answer
 * stays where it was wanted. The cost is one extra term in the right-hand
 * side.
 *
 * The linear algebra that follows is the price. J J^T is block tridiagonal
 * and symmetric positive definite, so it goes by block elimination with 6x6
 * blocks and needs no pivoting between blocks - only inside them.
 *
 * ---
 *
 * Scaling matters and is not optional. In metres and metres per second the
 * transition matrix has entries around 1e5 in the position-velocity block and
 * around 1 elsewhere, so J J^T spans ten orders of magnitude and the solve
 * loses most of its digits. The caller gives a length and a speed, the linear
 * algebra is done in those units, and the result is converted back. For the
 * Earth-Moon system the natural pair is the separation and the separation
 * times the orbital rate. */

#ifndef CORE_SHOOTING_H
#define CORE_SHOOTING_H

#include "integrator.h"

/* Doubles of workspace needed for n patch points. */
#define SHOOTING_WORKSPACE(n) (((size_t)(n)) * 84u + 64u)

typedef struct {
    double tol_m;          /* integrator tolerance for each leg */

    /* Converged when the worst position discontinuity falls below this.
     * Velocity continuity comes with it and is reported separately. */
    double continuity_m;

    double length_scale;   /* metres; see the note on scaling above */
    double speed_scale;    /* metres per second */

    int max_iterations;    /* 0 -> 20 */
} ShootingConfig;

typedef struct {
    int    iterations;
    double worst_position_gap;   /* metres, at the end */
    double worst_velocity_gap;   /* m/s, at the end */

    /* Largest position change any patch point took in a single iteration.
     * A gentle correction stays small throughout; a large value means Newton
     * took a jump, which is worth knowing even when it converges. The total
     * displacement from the initial guess is the caller's to measure, since
     * only the caller still has the guess. */
    double worst_step_m;
} ShootingReport;

/* Make states[0..n-1], at times[0..n-1], continuous under the dynamics of f.
 *
 * states is updated in place; times are fixed and are not unknowns. f is the
 * block form, because the transition matrix of each leg is what drives the
 * correction - pass accel_field_var for the ephemeris or accel_cr3bp_var for
 * the CR3BP.
 *
 * Returns CORE_ERR_TOLERANCE_NOT_MET if it does not converge, having left
 * states holding the best iterate reached, and report describing it. */
CoreResult shoot_multiple(BlockAccelFunc f, void *ctx,
                          State *states, const double *times, size_t n,
                          const ShootingConfig *cfg,
                          double *workspace, size_t workspace_len,
                          ShootingReport *report);

#endif /* CORE_SHOOTING_H */
