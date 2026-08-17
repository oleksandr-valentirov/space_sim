/* Linear covariance propagation for the uncertainty mechanic (PROJECT.md
 * section 8, ROADMAP "M6 — механіка невизначеності").
 *
 * The design bet PROJECT.md section 8 makes is that the vessel's state is an
 * ESTIMATE, not a fact - a probability cloud that grows between tracking
 * passes and shrinks after one. This file is the smallest piece of that
 * which is exact rather than sketched: given the state transition matrix
 * differential correction already builds (stm.h), a covariance propagates
 * forward by the standard linear (extended-Kalman-prediction) rule
 *
 *     P' = Phi P Phi^T
 *
 * which is valid in the same regime differential correction itself depends
 * on - the perturbation stays small enough that the dynamics are well
 * approximated by their linearisation over one propagation interval. Near an
 * unstable halo orbit that regime does not last forever, and that is not a
 * bug to work around: it is the point PROJECT.md section 8 is built on. When
 * this stops holding, the honest answer is "the linear estimate is no longer
 * trustworthy", which is itself useful to know.
 *
 * What this file does NOT do: no measurement model, no real orbit
 * determination, no filter gain computed from an actual tracking geometry.
 * A "tracking pass" is `uncertainty_scale` with a factor below one - a
 * single knob standing in for "some measurement happened and improved our
 * knowledge by roughly this much". That is deliberately out of scope here;
 * see ROADMAP.md, "M6 — механіка невизначеності", for the reasoning, and
 * core/export/ex_uncertainty.c for how
 * it is used to answer PROJECT.md's example question ("burn now, or wait
 * for one more pass"). */

#ifndef CORE_UNCERTAINTY_H
#define CORE_UNCERTAINTY_H

#include "stm.h"

/* Layout matches StmMatrix: 6x6, row-major, state ordered (x, y, z, vx, vy,
 * vz), so p[i * 6 + j] is Cov(y_i, y_j). Units follow the state: metres
 * squared and (metres/second) squared on the diagonal blocks, mixed units
 * off them. */

/* out = phi * p * phi^T. out may not alias p or phi.
 *
 * Symmetric in, symmetric out up to floating point - callers that want to
 * watch that can check with uncertainty_symmetry_defect. */
void uncertainty_propagate(const double phi[STM_SIZE], const double p[STM_SIZE],
                           double out[STM_SIZE]);

/* p *= factor, uniformly and in place. A tracking pass is factor in (0, 1);
 * the function does not enforce that range - a caller modelling something
 * else (a floor, a directional update) can build on top of it. */
void uncertainty_scale(double p[STM_SIZE], double factor);

/* sqrt(mean of the three position, resp. velocity, diagonal variances) - an
 * isotropic summary, not the true ellipsoid semi-axes. Those need
 * eigenvalues, which for a general symmetric 3x3 need libm (a trigonometric
 * solve of a cubic) - out of scope for code that stays in the deterministic
 * zone (CLAUDE.md invariant 3). Good enough to answer "is the cloud growing
 * or shrinking", not "which direction is it long in". */
double uncertainty_position_sigma(const double p[STM_SIZE]);
double uncertainty_velocity_sigma(const double p[STM_SIZE]);

/* max_ij |p_ij - p_ji|. A covariance is symmetric by construction; a nonzero
 * result here is floating-point drift worth watching over many propagation
 * steps, not evidence of a bug by itself. */
double uncertainty_symmetry_defect(const double p[STM_SIZE]);

#endif /* CORE_UNCERTAINTY_H */
