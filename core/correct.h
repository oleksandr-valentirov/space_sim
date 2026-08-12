/* Differential correction of periodic orbits (ROADMAP C2b).
 *
 * C2a reproduced somebody else's halo orbit. This finds one.
 *
 * The method is single shooting on a symmetry. Halo orbits are symmetric about
 * the xz-plane, so a state on that plane with the velocity perpendicular to it
 *
 *     (x, 0, z, 0, vy, 0)
 *
 * generates a periodic orbit if and only if the trajectory returns to the
 * plane with the velocity perpendicular again - that is, if vx and vz are zero
 * at the next crossing of y = 0. The other half of the orbit is then the
 * mirror image and needs no integrating. So a six-dimensional periodicity
 * condition collapses to two equations in two unknowns, and the period comes
 * out as twice the crossing time rather than being searched for.
 *
 * Two unknowns from three candidates (x, z, vy): one is held, which is what
 * picks a single orbit out of a one-parameter family. Holding z and varying
 * (x, vy) walks the family by out-of-plane amplitude, which is how halo orbits
 * are catalogued; holding x is the alternative for the parts of the family
 * where the first choice turns singular.
 *
 * The Jacobian of the two equations is not simply four entries of the
 * transition matrix, because the crossing time itself moves when the initial
 * state does. That correction term is the part most easily left out, and
 * leaving it out costs convergence rather than correctness - which is exactly
 * the failure ROADMAP C2b tells you to blame on the STM first. It is written
 * out in correct.c.
 *
 * No libm here: this is runtime-side code, and everything it needs is
 * +, -, *, / and a bracketed root search. */

#ifndef CORE_CORRECT_H
#define CORE_CORRECT_H

#include "cr3bp.h"
#include "integrator.h"
#include "stm.h"

typedef enum {
    /* Hold z, vary x and vy. The catalogue's own parameter. */
    HALO_HOLD_Z = 0,
    /* Hold x, vary z and vy. */
    HALO_HOLD_X = 1,
} HaloHold;

typedef struct {
    HaloHold hold;

    /* Converged when |vx| + |vz| at the crossing falls below this.
     * ROADMAP C2b asks for 1e-10. Dimensionless velocity units. */
    double tol;

    /* Tolerance handed to DOP853, in the same dimensionless units. Must be
     * comfortably below tol: the corrector cannot resolve a residual smaller
     * than the integrator's own error. */
    double integrator_tol;

    int max_iterations;   /* 0 -> 20 */

    /* Largest change to a free variable in one Newton step, 0 for no limit.
     * Newton from a good seed does not need it; Newton from a third-order
     * analytic approximation occasionally does, and an unlimited first step
     * can throw the trajectory into a primary, from which there is no
     * recovering. */
    double max_step;
} HaloCorrectConfig;

typedef struct {
    State  s;           /* corrected state at the crossing, t = 0 */
    double period;
    double jacobi;
    double residual;    /* |vx| + |vz| at the half-period crossing */
    int    iterations;
} HaloOrbit;

/* Correct a seed towards a periodic orbit.
 *
 * The seed's y, vx and vz are ignored and taken as zero: the symmetry is what
 * defines the family, and a seed that violates it is not a nearby orbit but a
 * different problem. period_guess only has to be close enough to bracket the
 * right crossing of y = 0; it is not used as an unknown.
 *
 * Returns CORE_ERR_TOLERANCE_NOT_MET if the iteration does not converge, the
 * crossing cannot be bracketed, or the 2x2 Jacobian turns singular. */
CoreResult halo_correct(double mu, const State *seed, double period_guess,
                        const HaloCorrectConfig *cfg, HaloOrbit *out);

/* Walk the family: step the held variable by `step` and re-converge, count
 * times, using each orbit as the seed for the next.
 *
 * This is what makes a corrector useful rather than merely correct - a single
 * orbit is an initial condition, a family is a design space. Stops early and
 * returns CORE_OK with a short count if an orbit fails to converge, since the
 * family ends somewhere and running off its end is expected, not exceptional.
 * The caller's buffer, as always (PROJECT.md section 5). */
CoreResult halo_family(double mu, const HaloOrbit *seed, double step,
                       const HaloCorrectConfig *cfg,
                       HaloOrbit *out, size_t cap, size_t *out_count);

#endif /* CORE_CORRECT_H */
