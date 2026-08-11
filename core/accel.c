#include "accel.h"

void accel_two_body(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out)
{
    (void)t;
    (void)v;

    const TwoBodyCtx *c = (const TwoBodyCtx *)ctx;

    double r2 = vec3_norm_sq(r);
    double rn = vec3_norm(r);

    /* -mu / |r|^3, formed as one division so the operation order is fixed. */
    double s = -c->mu / (r2 * rn);

    *a_out = vec3_scale(r, s);
}

double two_body_energy(Vec3d r, Vec3d v, double mu)
{
    return 0.5 * vec3_norm_sq(v) - mu / vec3_norm(r);
}

Vec3d two_body_angular_momentum(Vec3d r, Vec3d v)
{
    return vec3_cross(r, v);
}

double two_body_period(Vec3d r, Vec3d v, double mu)
{
    double energy = two_body_energy(r, v, mu);
    if (energy >= 0.0) {
        return 0.0;
    }

    /* E = -mu / (2a) */
    double a = -mu / (2.0 * energy);

    /* T = 2*pi*sqrt(a^3/mu). Written with sqrt only; PI is a literal because
     * this file is in the deterministic zone and libm is not available. */
    const double two_pi = 6.28318530717958647692;
    return two_pi * sqrt((a * a * a) / mu);
}
