#include "cr3bp.h"

double cr3bp_mu(double gm_primary, double gm_secondary)
{
    double total = gm_primary + gm_secondary;
    if (!(total > 0.0)) {
        return 0.0;
    }
    return gm_secondary / total;
}

/* Distances to the primary at (-mu, 0, 0) and the secondary at (1-mu, 0, 0). */
static void distances(Vec3d r, double mu, double *r1, double *r2)
{
    double dx1 = r.x + mu;
    double dx2 = r.x - (1.0 - mu);

    *r1 = sqrt(dx1 * dx1 + r.y * r.y + r.z * r.z);
    *r2 = sqrt(dx2 * dx2 + r.y * r.y + r.z * r.z);
}

double cr3bp_potential(Vec3d r, double mu)
{
    double r1, r2;
    distances(r, mu, &r1, &r2);

    return 0.5 * (r.x * r.x + r.y * r.y) + (1.0 - mu) / r1 + mu / r2;
}

double cr3bp_jacobi(Vec3d r, Vec3d v, double mu)
{
    return 2.0 * cr3bp_potential(r, mu) - vec3_norm_sq(v);
}

void accel_cr3bp(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out)
{
    (void)t;

    const Cr3bpCtx *c = (const Cr3bpCtx *)ctx;
    double mu = c->mu;

    double r1, r2;
    distances(r, mu, &r1, &r2);

    double r1_cubed = r1 * r1 * r1;
    double r2_cubed = r2 * r2 * r2;

    double a = (1.0 - mu) / r1_cubed;
    double b = mu / r2_cubed;

    double dx1 = r.x + mu;
    double dx2 = r.x - (1.0 - mu);

    /* Gravity plus the centrifugal term, which is what dOmega/dr is, then the
     * Coriolis term, which is the part that depends on velocity. */
    a_out->x = r.x - a * dx1 - b * dx2 + 2.0 * v.y;
    a_out->y = r.y - a * r.y - b * r.y - 2.0 * v.x;
    a_out->z = -a * r.z - b * r.z;
}

/* dOmega/dx along the x-axis, where y = z = 0. Its roots are L1, L2 and L3. */
static double collinear_gradient(double x, double mu)
{
    double dx1 = x + mu;
    double dx2 = x - (1.0 - mu);

    double r1 = dx1 < 0.0 ? -dx1 : dx1;
    double r2 = dx2 < 0.0 ? -dx2 : dx2;

    return x - (1.0 - mu) * dx1 / (r1 * r1 * r1)
             - mu * dx2 / (r2 * r2 * r2);
}

/* Bisection to full double precision. 200 iterations is far more than the
 * ~60 the interval needs, and costs nothing at setup time. */
static double bisect(double low, double high, double mu)
{
    double f_low = collinear_gradient(low, mu);

    for (int i = 0; i < 200; i++) {
        double mid = 0.5 * (low + high);
        double f_mid = collinear_gradient(mid, mu);

        if ((f_mid < 0.0) == (f_low < 0.0)) {
            low = mid;
            f_low = f_mid;
        } else {
            high = mid;
        }
    }

    return 0.5 * (low + high);
}

CoreResult cr3bp_lagrange(double mu, int point, Vec3d *out)
{
    if (out == NULL || !(mu > 0.0) || !(mu < 1.0) || point < 1 || point > 5) {
        return CORE_ERR_INVALID_ARG;
    }

    /* The gradient blows up at each primary, so the brackets stop just short
     * of them. Signs at the ends are opposite in every case, which is what
     * makes bisection safe here without any initial guess at all. */
    const double edge = 1e-9;

    switch (point) {
    case 1:
        *out = vec3(bisect(-mu + edge, (1.0 - mu) - edge, mu), 0.0, 0.0);
        return CORE_OK;

    case 2:
        *out = vec3(bisect((1.0 - mu) + edge, 5.0, mu), 0.0, 0.0);
        return CORE_OK;

    case 3:
        *out = vec3(bisect(-5.0, -mu - edge, mu), 0.0, 0.0);
        return CORE_OK;

    case 4:
    case 5: {
        /* Exact: the equilateral points. sqrt(3)/2 written as a literal
         * because it is a constant of the geometry, not something to derive. */
        const double sqrt3_over_2 = 0.86602540378443864676;
        double y = point == 4 ? sqrt3_over_2 : -sqrt3_over_2;
        *out = vec3(0.5 - mu, y, 0.0);
        return CORE_OK;
    }

    default:
        return CORE_ERR_INVALID_ARG;
    }
}
