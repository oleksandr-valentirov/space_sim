#include "cr3bp.h"

#include "stm.h"

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

#define ZVC_SCAN_SAMPLES 400
#define ZVC_BISECT_ITERATIONS 60

static double two_omega_minus_c(Vec3d r, double mu, double c)
{
    return 2.0 * cr3bp_potential(r, mu) - c;
}

CoreResult cr3bp_zvc_radius(double mu, double c, Vec3d from, Vec3d dir_unit,
                            double r_max, double *r_out)
{
    if (r_out == NULL || !(r_max > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    double step = r_max / (double)ZVC_SCAN_SAMPLES;
    double prev_r = 0.0;
    int sign_prev = two_omega_minus_c(from, mu, c) < 0.0;

    for (int i = 1; i <= ZVC_SCAN_SAMPLES; i++) {
        double r = step * (double)i;
        Vec3d p = vec3_add_scaled(from, dir_unit, r);
        int sign = two_omega_minus_c(p, mu, c) < 0.0;

        if (sign != sign_prev) {
            double lo = prev_r, hi = r;
            for (int k = 0; k < ZVC_BISECT_ITERATIONS; k++) {
                double mid = 0.5 * (lo + hi);
                Vec3d pm = vec3_add_scaled(from, dir_unit, mid);
                if ((two_omega_minus_c(pm, mu, c) < 0.0) == sign_prev) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            *r_out = 0.5 * (lo + hi);
            return CORE_OK;
        }

        prev_r = r;
        sign_prev = sign;
    }

    return CORE_ERR_TOLERANCE_NOT_MET;
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

void cr3bp_hessian(Vec3d r, double mu, double u[9])
{
    double r1, r2;
    distances(r, mu, &r1, &r2);

    double dx1 = r.x + mu;
    double dx2 = r.x - (1.0 - mu);

    double r1_sq = r1 * r1;
    double r2_sq = r2 * r2;

    double a = (1.0 - mu) / (r1_sq * r1);
    double b = mu / (r2_sq * r2);

    /* The 3/r^5 terms. Written as (1/r^3)/r^2 rather than as a fifth power so
     * the intermediate stays in the same range as the acceleration's own
     * terms; at a near-rectilinear perilune r2 reaches 7e-5 units, where r2^5
     * is 1.7e-21 and the difference in conditioning is not academic. */
    double a5 = 3.0 * a / r1_sq;
    double b5 = 3.0 * b / r2_sq;

    /* Omega = (x^2 + y^2)/2 + (1-mu)/r1 + mu/r2. The leading 1 in the xx and
     * yy terms is the centrifugal part; there is none in zz, which is the
     * structural reason the out-of-plane motion is oscillatory and the
     * in-plane motion is not. */
    u[0] = 1.0 - a - b + a5 * dx1 * dx1 + b5 * dx2 * dx2;
    u[4] = 1.0 - a - b + a5 * r.y * r.y + b5 * r.y * r.y;
    u[8] =     - a - b + a5 * r.z * r.z + b5 * r.z * r.z;

    u[1] = a5 * dx1 * r.y + b5 * dx2 * r.y;
    u[2] = a5 * dx1 * r.z + b5 * dx2 * r.z;
    u[5] = a5 * r.y * r.z + b5 * r.y * r.z;

    u[3] = u[1];
    u[6] = u[2];
    u[7] = u[5];
}

void accel_cr3bp_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out)
{
    accel_cr3bp(t, r[0], v[0], ctx, &a_out[0]);

    if (n_blocks < 2) {
        return;
    }

    const Cr3bpCtx *c = (const Cr3bpCtx *)ctx;

    double u[9];
    cr3bp_hessian(r[0], c->mu, u);

    for (int b = 1; b < n_blocks; b++) {
        Vec3d dr = r[b];
        Vec3d dv = v[b];

        a_out[b].x = u[0] * dr.x + u[1] * dr.y + u[2] * dr.z + 2.0 * dv.y;
        a_out[b].y = u[3] * dr.x + u[4] * dr.y + u[5] * dr.z - 2.0 * dv.x;
        a_out[b].z = u[6] * dr.x + u[7] * dr.y + u[8] * dr.z;
    }
}

void cr3bp_stm_canonical(const double phi_v[36], double phi_p[36])
{
    /* p = C y and y = C_inv p, so Phi_p = C Phi_v C_inv. Both are unit
     * triangular, hence exactly invertible in floating point - C C_inv comes
     * out bit-exactly the identity. */
    static const double c[STM_SIZE] = {
        1.0,  0.0, 0.0, 0.0, 0.0, 0.0,
        0.0,  1.0, 0.0, 0.0, 0.0, 0.0,
        0.0,  0.0, 1.0, 0.0, 0.0, 0.0,
        0.0, -1.0, 0.0, 1.0, 0.0, 0.0,
        1.0,  0.0, 0.0, 0.0, 1.0, 0.0,
        0.0,  0.0, 0.0, 0.0, 0.0, 1.0,
    };
    static const double c_inv[STM_SIZE] = {
         1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
         0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
         0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
         0.0, 1.0, 0.0, 1.0, 0.0, 0.0,
        -1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
         0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    };

    double tmp[STM_SIZE];
    stm_multiply(c, phi_v, tmp);
    stm_multiply(tmp, c_inv, phi_p);
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
