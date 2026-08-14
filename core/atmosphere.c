#include "atmosphere.h"

/* US Standard Atmosphere 1976 in Vallado's layered form (table 8-4). Altitudes
 * and scale heights are given there in kilometres and converted here rather
 * than in a caller, so that nothing outside this file has to remember which
 * unit the table used.
 *
 * The bands are uneven on purpose - ten kilometres apart where the profile
 * bends hardest, a hundred where it has settled - and that is a property of
 * the published fit, not a choice made here. */
const AtmosphereModel ATMOSPHERE_EARTH_USSA76 = {
    28,
    {
        {      0.0, 1.225e+00,  7249.0 },
        {  25000.0, 3.899e-02,  6349.0 },
        {  30000.0, 1.774e-02,  6682.0 },
        {  40000.0, 3.972e-03,  7554.0 },
        {  50000.0, 1.057e-03,  8382.0 },
        {  60000.0, 3.206e-04,  7714.0 },
        {  70000.0, 8.770e-05,  6549.0 },
        {  80000.0, 1.905e-05,  5799.0 },
        {  90000.0, 3.396e-06,  5382.0 },
        { 100000.0, 5.297e-07,  5877.0 },
        { 110000.0, 9.661e-08,  7263.0 },
        { 120000.0, 2.438e-08,  9473.0 },
        { 130000.0, 8.484e-09, 12636.0 },
        { 140000.0, 3.845e-09, 16149.0 },
        { 150000.0, 2.070e-09, 22523.0 },
        { 180000.0, 5.464e-10, 29740.0 },
        { 200000.0, 2.789e-10, 37105.0 },
        { 250000.0, 7.248e-11, 45546.0 },
        { 300000.0, 2.418e-11, 53628.0 },
        { 350000.0, 9.518e-12, 53298.0 },
        { 400000.0, 3.725e-12, 58515.0 },
        { 450000.0, 1.585e-12, 60828.0 },
        { 500000.0, 6.967e-13, 63822.0 },
        { 600000.0, 1.454e-13, 71835.0 },
        { 700000.0, 3.614e-14, 88667.0 },
        { 800000.0, 1.170e-14, 124640.0 },
        { 900000.0, 5.245e-15, 181050.0 },
        { 1000000.0, 3.019e-15, 268000.0 },
    },
};

double atmosphere_exp_neg(double x)
{
    /* Written so that NaN falls through rather than being answered. The
     * obvious !(x > 0.0) would return 1.0 for a NaN altitude and hide it
     * inside a plausible density; here the comparison fails, the reduction
     * loop declines to run, and the series carries the NaN out where the
     * field's sticky failure flag and the integrator can see it. */
    if (x <= 0.0) {
        return 1.0;
    }

    /* Range reduction by halving: exp(-x) = (exp(-x/2))^2, applied until the
     * argument lands in (0, 1] where the series converges fastest.
     *
     * Halving rather than splitting off an integer part, which is the usual
     * trick, because halving needs no additional constant - multiplying by
     * 0.5 is exact - while the integer-part form needs exp(-1) to a precision
     * we would then have to justify. Six squarings multiply the relative error
     * by 64, which starting from 1e-16 is still nothing. */
    int m = 0;
    while (x > 1.0) {
        if (m >= ATMOSPHERE_EXP_HALVINGS) {
            return 0.0;
        }
        x *= 0.5;
        m++;
    }

    /* exp(+x) by Horner on the Taylor series, then one reciprocal.
     *
     * The positive series and a division, rather than the alternating series
     * for exp(-x) directly: at x = 1 the alternating terms reach 1 while their
     * sum is 0.368, so a digit is lost to cancellation for nothing. Every term
     * here is positive.
     *
     * The coefficients are 1/k! and are not written out. Dividing by the loop
     * index reproduces them exactly and can be read as the series it is,
     * whereas a table of seventeen literals would be seventeen chances to
     * mistype a factorial and no way to see it. The evaluation order is fixed
     * and part of the result's last bits (PROJECT.md section 4). */
    double s = 1.0;
    for (int k = ATMOSPHERE_EXP_TERMS; k >= 1; k--) {
        s = 1.0 + (x / (double)k) * s;
    }

    double e = 1.0 / s;
    while (m-- > 0) {
        e = e * e;
    }
    return e;
}

void atmosphere_density(const AtmosphereModel *m, double altitude_m,
                        double *rho_out, double *drho_dh_out)
{
    double rho = 0.0;
    double drho = 0.0;

    if (m == 0 || m->n_layers <= 0) {
        goto done;
    }

    /* Below the table: hold the bottom band's base value. See the header for
     * why this is a clamp and not an extrapolation. */
    if (altitude_m < m->layer[0].base_altitude_m) {
        altitude_m = m->layer[0].base_altitude_m;
    }

    /* Downward from the top, so the common case - a vessel in orbit, in the
     * last band or near it - is found in the first few comparisons. */
    int i = m->n_layers - 1;
    while (i > 0 && altitude_m < m->layer[i].base_altitude_m) {
        i--;
    }

    const AtmosphereLayer *l = &m->layer[i];
    if (!(l->scale_height_m > 0.0)) {
        goto done;   /* unusable band: vacuum, not a division by zero */
    }

    double x = (altitude_m - l->base_altitude_m) / l->scale_height_m;
    rho = l->base_density * atmosphere_exp_neg(x);

    /* d/dh of rho0 * exp(-(h-h0)/H) is -rho/H. Taken from the value just
     * computed rather than evaluated a second time: the two must agree
     * exactly, or a finite-difference check of the gradient would be
     * comparing against a slightly different density profile. */
    drho = -rho / l->scale_height_m;

done:
    if (rho_out != 0) {
        *rho_out = rho;
    }
    if (drho_dh_out != 0) {
        *drho_dh_out = drho;
    }
}

void drag_accel(double density, double coeff, Vec3d v_rel, Vec3d *a_out)
{
    if (!(density > 0.0) || !(coeff > 0.0)) {
        *a_out = vec3_zero();
        return;
    }

    double v2 = vec3_norm_sq(v_rel);
    if (!(v2 > 0.0)) {
        *a_out = vec3_zero();
        return;
    }

    double v = sqrt(v2);
    *a_out = vec3_scale(v_rel, -0.5 * density * coeff * v);
}

void drag_jacobian(double density, double drho_dh, double coeff,
                   Vec3d v_rel, Vec3d up, double dadr[9], double dadv[9])
{
    int k;

    if (dadr != 0) {
        for (k = 0; k < 9; k++) {
            dadr[k] = 0.0;
        }
    }
    if (dadv != 0) {
        for (k = 0; k < 9; k++) {
            dadv[k] = 0.0;
        }
    }

    if (!(coeff > 0.0)) {
        return;
    }

    double v2 = vec3_norm_sq(v_rel);
    if (!(v2 > 0.0)) {
        /* At rest in the air both Jacobians vanish. d(a)/d(v) has a 0/0 in it
         * written naively, but the limit is zero: the |v| in front of the
         * bracket goes faster than the v v^T / |v| inside it blows up. */
        return;
    }

    double v = sqrt(v2);

    if (dadv != 0 && density > 0.0) {
        double s = -0.5 * density * coeff;

        /* s * ( |v| I + v v^T / |v| ). Symmetric, and written as the upper
         * triangle mirrored so that a caller entitled to symmetry gets it to
         * the last bit rather than to within the order of two products. */
        double sv = s * v;
        double sov = s / v;

        dadv[0] = sv + sov * v_rel.x * v_rel.x;
        dadv[4] = sv + sov * v_rel.y * v_rel.y;
        dadv[8] = sv + sov * v_rel.z * v_rel.z;

        dadv[1] = sov * v_rel.x * v_rel.y;
        dadv[2] = sov * v_rel.x * v_rel.z;
        dadv[5] = sov * v_rel.y * v_rel.z;

        dadv[3] = dadv[1];
        dadv[6] = dadv[2];
        dadv[7] = dadv[5];
    }

    if (dadr != 0) {
        /* Position enters only through the density, so this is the outer
         * product of the velocity with the local vertical - rank one, and
         * asymmetric unless the vessel happens to fly straight up. No
         * mirroring here, and none is wanted. */
        double g = -0.5 * coeff * v * drho_dh;

        dadr[0] = g * v_rel.x * up.x;
        dadr[1] = g * v_rel.x * up.y;
        dadr[2] = g * v_rel.x * up.z;
        dadr[3] = g * v_rel.y * up.x;
        dadr[4] = g * v_rel.y * up.y;
        dadr[5] = g * v_rel.y * up.z;
        dadr[6] = g * v_rel.z * up.x;
        dadr[7] = g * v_rel.z * up.y;
        dadr[8] = g * v_rel.z * up.z;
    }
}
