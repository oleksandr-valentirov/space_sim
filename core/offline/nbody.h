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
#include "harmonics.h"
#include "integrator.h"

#include <stddef.h>

#define NBODY_MAX 16

typedef struct {
    size_t n;
    double mu[NBODY_MAX];   /* m^3/s^2, in the same order as the states */

    /* Each body's shape, or NULL for a point mass (ROADMAP K5e; one slot
     * for one body before that, when Earth's J2 was the only one). NULL
     * everywhere is what a zero-initialised struct gets, so a caller that
     * sets none behaves exactly as it did before K2 existed, bit for bit.
     *
     * BORROWED, not held by value, for the reason K5a moved FieldCtx the
     * same way: at degree 50 a HarmonicsField is 21 kB, sixteen of them are
     * a third of a megabyte, and this struct is a local variable in the
     * cooker and in three tests.
     *
     * THESE ARE TRUNCATED ON PURPOSE, and the caller does the truncating.
     * What acts between two bodies is not what a vessel skimming the surface
     * feels: the Moon's degree-2 term is 2e-5 of its point mass at the
     * Earth's distance and degree 4 is 4e-10 of it, so the cooker hands over
     * a low-degree copy while the asset carries the whole model
     * (core/cook/cook_fixture.c). The alternative - evaluating degree 50 on
     * every pair at every stage - costs 12 us a call to compute nothing. */
    const HarmonicsField *field[NBODY_MAX];

    /* Body names, for one purpose only: looking up the rotation model that
     * says which way a field with tesseral terms is pointing (ROADMAP K5e).
     * NULL means "no model", which is also what an unnamed body gets, and a
     * body whose field is zonal does not care either way.
     *
     * MEASURED, not assumed to matter: applying the Moon's own field with
     * its pole taken along the frame's z axis - the assumption K2 could
     * afford for the Earth, whose pole IS along z - moved the geocentric
     * lunar position 199 m over the fixture's span and made the error
     * against JPL WORSE, 2108 m to 2307 m. The Moon's pole is 23.46 degrees
     * off z. A field in the wrong frame is not a smaller correction than no
     * field; it is a different, wrong one. */
    const char *name[NBODY_MAX];
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

/* Velocity of that barycentre - the system's net momentum per unit of the mass
 * convention used here. Zero for the whole solar system in the SSB frame, and
 * not zero for any subset of it. */
Vec3d nbody_momentum_velocity(const NBodySystem *sys, const State *states);

/* Subtract that velocity from every body, so the system's barycentre stops
 * moving through the frame.
 *
 * Measured on the ten-body set at J2000: 3.35 mm/s of residual barycentre
 * velocity, which over ten years is 1.057e6 m of drift - against a measured
 * 1.051e6 m, so the drift is that momentum and nothing else. It is an
 * artefact rather than motion: the true solar system's barycentre is at rest
 * by construction, and ours moves only because a few hundred asteroids' worth
 * of mass and momentum is missing from the set, plus whatever the barycentre
 * GM question in data/horizons/README.md is worth. Choosing the frame in
 * which our own incomplete system is at rest costs nothing physical and
 * removes an error that grows without bound.
 *
 * Only the velocity. The barycentre also sits 3.4e5 m off the origin at
 * J2000, and that offset is deliberately left alone: a constant offset is not
 * an error that accumulates, and removing it would move every body away from
 * the published initial conditions, which are the best data available. The
 * drift is the artefact; where the subset's centre of mass happens to be is
 * not. */
void nbody_anchor_barycentre(const NBodySystem *sys, State *states);

/* DOP853 over the whole system. Same coefficients and same controller as the
 * runtime integrator, applied to n bodies at once; the error norm runs over
 * every position and velocity component of every body. */
CoreResult nbody_integrate(const NBodySystem *sys, const State *in,
                           double t_end, const Dop853Config *cfg,
                           Dop853State *io, State *out);

#endif /* CORE_NBODY_H */
