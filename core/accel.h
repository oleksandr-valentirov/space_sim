/* Acceleration models.
 *
 * The signature takes both position and velocity even though gravity needs
 * only position. That is deliberate and settled: drag goes as v^2, thrust
 * pointed along the velocity vector depends on v, and the drag component of
 * solar radiation pressure does too (PROJECT.md section 4). An acceleration
 * interface without v would have to be rewritten the moment a vessel enters
 * an atmosphere, and every integrator built on it with it. */

#ifndef CORE_ACCEL_H
#define CORE_ACCEL_H

#include "vec3.h"

/* t is seconds from the ephemeris epoch, r and v are barycentric in metres
 * and m/s. ctx carries whatever the model needs. */
typedef void (*AccelFunc)(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out);

typedef struct {
    double mu;   /* m^3/s^2 */
} TwoBodyCtx;

/* a = -mu * r / |r|^3, with the attractor at the origin.
 *
 * Not the general case, and not meant to be: it is the one problem with a
 * closed-form solution, which makes it the only place where the integrator
 * can be measured against an exact answer rather than against another
 * integrator. */
void accel_two_body(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out);

/* Specific orbital energy, v^2/2 - mu/r. Conserved exactly by the true
 * dynamics, so its drift measures the integrator rather than the physics. */
double two_body_energy(Vec3d r, Vec3d v, double mu);

/* Specific angular momentum, r x v. Also conserved, and it fails differently
 * from energy: a sign error in the force leaves energy plausible while
 * angular momentum collapses. */
Vec3d two_body_angular_momentum(Vec3d r, Vec3d v);

/* Orbital period from the vis-viva semi-major axis. Returns 0 for unbound
 * orbits rather than a NaN, so a caller that forgets to check gets an
 * obviously wrong number instead of a silent poison value. */
double two_body_period(Vec3d r, Vec3d v, double mu);

#endif /* CORE_ACCEL_H */
