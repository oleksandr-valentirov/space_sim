/* Circular restricted three-body problem (ROADMAP C1).
 *
 * A deliberately simplified model, and the reason it exists is that it has
 * things the full ephemeris does not: analytic equilibrium points, a
 * conserved quantity, and a published catalogue of periodic orbits. Those are
 * what make it possible to tell a correct integrator from a plausible one.
 * The halo orbit that Milestone 0 turns on is found here first, and only then
 * carried into the real ephemeris.
 *
 * Dimensionless throughout, in the usual normalisation: the two primaries are
 * one unit apart, their total mass is 1, and the frame rotates with them at
 * unit angular velocity, so the secondary's period is 2*pi. The primary of
 * mass 1-mu sits at (-mu, 0, 0) and the secondary of mass mu at (1-mu, 0, 0).
 *
 * Note what the equations of motion need: the Coriolis term depends on
 * velocity. This is the first user of AccelFunc that actually requires the v
 * argument, which existed from B3 for a reason (PROJECT.md section 4). */

#ifndef CORE_CR3BP_H
#define CORE_CR3BP_H

#include "accel.h"
#include "core.h"

typedef struct {
    double mu;   /* m2 / (m1 + m2), dimensionless */
} Cr3bpCtx;

/* mu from two gravitational parameters, in any consistent unit. */
double cr3bp_mu(double gm_primary, double gm_secondary);

/* Acceleration in the rotating frame, including centrifugal and Coriolis
 * terms. Signature matches AccelFunc so the existing integrators drive it. */
void accel_cr3bp(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out);

/* The effective potential Omega = (x^2 + y^2)/2 + (1-mu)/r1 + mu/r2. */
double cr3bp_potential(Vec3d r, double mu);

/* Jacobi constant C = 2*Omega - v^2.
 *
 * The one conserved quantity of the problem, and therefore the sharpest
 * available measure of an integrator: the true dynamics hold it exactly, so
 * every digit it loses was lost by the numerics. */
double cr3bp_jacobi(Vec3d r, Vec3d v, double mu);

/* Lagrange points, 1 to 5.
 *
 * L4 and L5 are exact: the equilateral points at (1/2 - mu, +-sqrt(3)/2, 0).
 * L1 to L3 are found by bisection on dOmega/dx along the x-axis - slow, but
 * this runs at setup and never in a loop, it cannot diverge, and it needs no
 * cube root, which matters because pow() is not available here. */
CoreResult cr3bp_lagrange(double mu, int point, Vec3d *out);

#endif /* CORE_CR3BP_H */
