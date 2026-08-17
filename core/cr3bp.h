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

/* Zero-velocity curve: the boundary of the region a Jacobi constant c makes
 * unreachable (PROJECT.md section 7, "Карта — це і є наша графіка"; ROADMAP.md
 * G4). v^2 = 2*Omega - c, so v^2 < 0 - impossible - exactly where
 * 2*Omega(r) < c: that inequality, not any curve-tracing, is the whole
 * physics here. This function finds where it turns into an equality along
 * one ray, which is what a caller sweeping many rays needs to draw the
 * boundary as a polyline.
 *
 * Scans from `from` outward along dir_unit (a unit vector - the caller
 * turns an angle into one with the one cos/sin pair this file cannot have,
 * CLAUDE.md invariant 3) for the first r in (0, r_max] where the sign of
 * 2*Omega(from + r*dir_unit) - c changes, then bisects it. Plain bisection,
 * not Newton, on purpose: cr3bp_lagrange below already makes the case for
 * it in this file - this runs a handful of times per rendered curve, never
 * in a hot loop, and cannot diverge.
 *
 * Returns CORE_ERR_INVALID_ARG for r_max <= 0. Returns
 * CORE_ERR_TOLERANCE_NOT_MET if the whole ray from `from` to
 * `from + r_max * dir_unit` stays on one side of the boundary - at this c,
 * the region along that ray is either entirely forbidden or entirely open,
 * which is itself the topology answer for that ray. */
CoreResult cr3bp_zvc_radius(double mu, double c, Vec3d from, Vec3d dir_unit,
                            double r_max, double *r_out);

/* Lagrange points, 1 to 5.
 *
 * L4 and L5 are exact: the equilateral points at (1/2 - mu, +-sqrt(3)/2, 0).
 * L1 to L3 are found by bisection on dOmega/dx along the x-axis - slow, but
 * this runs at setup and never in a loop, it cannot diverge, and it needs no
 * cube root, which matters because pow() is not available here. */
CoreResult cr3bp_lagrange(double mu, int point, Vec3d *out);

/* Second derivatives of the effective potential, row-major 3x3 and symmetric:
 * u[i*3+j] = d2 Omega / d r_i d r_j.
 *
 * This is the whole content of the variational equations - everything else
 * about them is the same Coriolis term the state already has, because that
 * part of the dynamics is already linear in velocity. */
void cr3bp_hessian(Vec3d r, double mu, double u[9]);

/* Accelerations for a reference trajectory in block 0 and up to six
 * linearised companions in blocks 1..n-1, in the form
 * dop853_integrate_blocks wants. ctx is a Cr3bpCtx, as for accel_cr3bp.
 *
 * Companion b is a perturbation (dr, dv) about block 0, and its acceleration
 * is Omega_rr(r0) dr plus the Coriolis term applied to dv. Note that it is
 * evaluated at block 0's position at that stage, not at the start of the step:
 * that is exactly why the STM has to share the integrator's stages rather than
 * run beside it on its own. */
void accel_cr3bp_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out);

/* Re-express a transition matrix from (position, velocity) coordinates in
 * (position, conjugate momentum) ones. Both 6x6 row-major; in and out may not
 * alias.
 *
 * This exists because of a trap that cost a wrong conclusion here. The CR3BP
 * is Hamiltonian, so its transition matrix must be symplectic - and the one
 * stm_integrate produces is not, because in a rotating frame the momentum
 * conjugate to position is not the velocity:
 *
 *     px = vx - y,   py = vy + x,   pz = vz
 *
 * Checking Phi in velocity coordinates against Phi^T J Phi = J gives a defect
 * of 60 where the entries are 10, and - the part that identifies it as a
 * definition error rather than a numerical one - the defect does not move at
 * all when the tolerance is tightened by three orders of magnitude. After the
 * change of variables the defect falls from 7.5e-9 to 3.9e-12 as the tolerance
 * goes from 1e-12 to 1e-15, which is what an integration error looks like.
 *
 * No transformation is needed in an inertial frame, where momentum per unit
 * mass is velocity. This is a rotating-frame matter, which is why it lives
 * here and not in stm.c. */
void cr3bp_stm_canonical(const double phi_v[36], double phi_p[36]);

#endif /* CORE_CR3BP_H */
