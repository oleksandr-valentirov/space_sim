/* Mutual N-body integration - OFFLINE ONLY (ROADMAP B5).
 *
 * This is what the ephemeris cooker runs: the planets and moons pulling on
 * each other, integrated once on the developer's machine and baked into a
 * versioned asset (PROJECT.md section 4). The runtime never does this - it
 * reads Chebyshev coefficients and treats vessels as massless test particles
 * in the field those coefficients describe.
 *
 * Which is why it lives here rather than beside dop853.c. The runtime
 * integrator works on one State because that is the shape of the runtime
 * problem; duplicating its stage arithmetic for a different data shape is
 * cheaper than generalising a hot path that is already tested and hashed. */

#ifndef CORE_NBODY_H
#define CORE_NBODY_H

#include "core.h"
#include "integrator.h"

#include <stddef.h>

#define NBODY_MAX 16

typedef struct {
    size_t n;
    double mu[NBODY_MAX];   /* m^3/s^2, in the same order as the states */
} NBodySystem;

/* Accelerations of every body from every other.
 *
 * Each body's sum runs over the others in index order, and the pairwise force
 * is computed twice rather than once and negated. That doubles the
 * arithmetic, which is irrelevant at these sizes, and buys an accumulation
 * order that is obvious from reading the loop. */
void nbody_accel(const NBodySystem *sys, const State *states, Vec3d *acc_out);

/* Total energy of the system, kinetic plus pairwise potential, per unit of
 * the mass convention used here (mu instead of mass). Conserved by the true
 * dynamics, so its drift measures the integrator. */
double nbody_energy(const NBodySystem *sys, const State *states);

/* Barycentre of the system. With every body of the solar system present this
 * would sit at the origin of the frame the fixtures use; with a subset it
 * does not, and watching it move is one way to see what the subset is
 * missing. */
Vec3d nbody_barycentre(const NBodySystem *sys, const State *states);

/* DOP853 over the whole system. Same coefficients and same controller as the
 * runtime integrator, applied to n bodies at once; the error norm runs over
 * every position and velocity component of every body. */
CoreResult nbody_integrate(const NBodySystem *sys, const State *in,
                           double t_end, const Dop853Config *cfg,
                           Dop853State *io, State *out);

#endif /* CORE_NBODY_H */
