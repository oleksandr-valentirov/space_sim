/* Lambert's problem: the velocities of the orbit that flies from r1 to r2 in
 * time dt (PROJECT.md section 5, sketch under "Планування").
 *
 * This lives outside the determinism boundary (PROJECT.md section 4,
 * ROADMAP.md "Далі - грубо", M3): a maneuver the player commits to is a
 * state - (time, delta-v) - to hand to the propagator, not a trajectory that
 * has to reproduce bit-for-bit across platforms. That is what lets this file
 * sit under core/planning and call libm freely, unlike everything directly
 * under core/. scripts/check_no_libm.sh only scans build/core at its top
 * level for exactly this reason: core/planning is deliberately out of reach.
 *
 * Solved with the universal-variable formulation (Curtis, "Orbital Mechanics
 * for Engineering Students", algorithm 5.2): one parameter z unifies the
 * elliptic, parabolic and hyperbolic transfer orbit into a single root-find,
 * solved with Newton's method from z = 0.
 *
 * Zero-revolution transfers only. The multi-revolution case brackets more
 * than one root of the same time-of-flight equation and needs its own
 * bisection scheme to pick a branch; PROJECT.md's sketch reserves n_revs for
 * it, but nothing here implements it yet. */

#ifndef CORE_PLANNING_LAMBERT_H
#define CORE_PLANNING_LAMBERT_H

#include "core.h"

/* Solve for the two velocities of the transfer orbit connecting r1 to r2 in
 * time dt > 0, around a body of gravitational parameter mu > 0.
 *
 * prograde selects which of the two transfer angles (theta or 2*pi - theta,
 * where theta = acos(r1 . r2 / (|r1||r2|)) is direction-agnostic) the
 * trajectory takes: nonzero for the branch whose motion has r1 x r2 pointing
 * toward +z, zero for the other one. This is the standard convention for a
 * frame whose z is the transfer plane's normal (e.g. the ecliptic pole for
 * interplanetary transfers); a caller working in a different plane rotates
 * r1, r2 into one where that holds before calling.
 *
 * n_revs must be 0 (see file comment).
 *
 * Returns CORE_ERR_INVALID_ARG for dt <= 0, mu <= 0, n_revs != 0, or
 * geometry with r1 and r2 collinear (through the origin) - the transfer
 * plane and hence the direction convention above are undefined there.
 * Returns CORE_ERR_TOLERANCE_NOT_MET if Newton's method leaves the domain
 * where the universal-variable formulas are valid, or does not converge
 * within its iteration budget; core/test/test_lambert.c records which
 * geometries this actually happens for. */
CoreResult lambert_solve(Vec3d r1, Vec3d r2, double dt, double mu,
                         int prograde, int n_revs,
                         Vec3d *v1_out, Vec3d *v2_out);

#endif /* CORE_PLANNING_LAMBERT_H */
