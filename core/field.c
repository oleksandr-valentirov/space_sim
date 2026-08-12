#include "field.h"

#include <string.h>

CoreResult field_all_bodies(const EphemerisCtx *eph, FieldCtx *out)
{
    return field_all_but(eph, -1, out);
}

CoreResult field_all_but(const EphemerisCtx *eph, int excluded, FieldCtx *out)
{
    if (eph == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    int count = eph_body_count(eph);
    if (count <= 0) {
        return CORE_ERR_INVALID_ARG;
    }
    if (count > FIELD_MAX_BODIES) {
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    memset(out, 0, sizeof *out);
    out->eph = eph;

    for (int i = 0; i < count; i++) {
        if (i == excluded) {
            continue;
        }
        out->body[out->n_bodies++] = i;
    }

    if (out->n_bodies == 0) {
        return CORE_ERR_INVALID_ARG;
    }

    return CORE_OK;
}

/* Position of body index b at t, and the vector from r to it. Returns 0 and
 * trips the failure flag if the ephemeris cannot answer. */
static int offset_to_body(FieldCtx *c, int b, double t, Vec3d r, Vec3d *out)
{
    State s;
    if (eph_body_state(c->eph, c->body[b], t, &s) != CORE_OK) {
        c->failed = 1;
        return 0;
    }

    *out = vec3_sub(s.r, r);
    return 1;
}

void accel_field(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out)
{
    (void)v;

    FieldCtx *c = (FieldCtx *)ctx;
    Vec3d sum = vec3_zero();

    /* Index order, always. A reordering here is a different trajectory
     * (vec3.h), and this is the sum most likely to be "optimised" later by
     * sorting bodies by distance. */
    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d)) {
            *a_out = vec3_zero();
            return;
        }

        double r2 = vec3_norm_sq(d);
        double rn = sqrt(r2);
        double inv_r3 = 1.0 / (r2 * rn);

        sum = vec3_add_scaled(sum, d, eph_body_mu(c->eph, c->body[i]) * inv_r3);
    }

    *a_out = sum;
}

void field_gradient(double t, Vec3d r, const FieldCtx *ctx, double g[9])
{
    /* Const in the signature because a gradient is a measurement, not a step;
     * the cast is to reuse offset_to_body, and the only state it touches is
     * the failure flag, which a failed gradient should set too. */
    FieldCtx *c = (FieldCtx *)ctx;

    for (int k = 0; k < 9; k++) {
        g[k] = 0.0;
    }

    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d)) {
            for (int k = 0; k < 9; k++) {
                g[k] = 0.0;
            }
            return;
        }

        double r2 = vec3_norm_sq(d);
        double rn = sqrt(r2);
        double inv_r3 = 1.0 / (r2 * rn);
        double mu = eph_body_mu(c->eph, c->body[i]);

        double a = mu * inv_r3;
        double b = 3.0 * a / r2;   /* 3 mu / |d|^5, kept as (mu/|d|^3)/|d|^2 */

        /* Only the upper triangle is computed, and the lower is copied from
         * it. Not for speed: b*dy*dz and b*dz*dy are equal in exact
         * arithmetic and differ in the last bit in floating point, because
         * they associate differently. Computing both would make the matrix
         * asymmetric at the 1e-16 level, and a caller entitled to assume
         * symmetry would be quietly wrong. */
        g[0] += b * d.x * d.x - a;
        g[4] += b * d.y * d.y - a;
        g[8] += b * d.z * d.z - a;

        g[1] += b * d.x * d.y;
        g[2] += b * d.x * d.z;
        g[5] += b * d.y * d.z;
    }

    g[3] = g[1];
    g[6] = g[2];
    g[7] = g[5];
}

void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out)
{
    accel_field(t, r[0], v[0], ctx, &a_out[0]);

    if (n_blocks < 2) {
        return;
    }

    FieldCtx *c = (FieldCtx *)ctx;

    double g[9];
    field_gradient(t, r[0], c, g);

    for (int b = 1; b < n_blocks; b++) {
        Vec3d dr = r[b];

        a_out[b].x = g[0] * dr.x + g[1] * dr.y + g[2] * dr.z;
        a_out[b].y = g[3] * dr.x + g[4] * dr.y + g[5] * dr.z;
        a_out[b].z = g[6] * dr.x + g[7] * dr.y + g[8] * dr.z;
    }
}
