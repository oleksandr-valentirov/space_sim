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

        int slot = out->n_bodies++;
        out->body[slot] = i;

        /* Read once, here, rather than on every force evaluation: the
         * coefficients do not change, and the alternative is a file-format
         * decision reached inside the integrator's inner loop. */
        if (eph_body_harmonics(eph, i, &out->harmonics[slot]) != CORE_OK) {
            return CORE_ERR_INVALID_ARG;
        }
        if (out->harmonics[slot].degree >= 2) {
            out->n_harmonic++;
        }

        /* Same argument, same place: read once here, never inside a force
         * evaluation. Zero for either is a legitimate answer from the asset
         * (core/ephemeris.h), not a failure. */
        out->radius[slot] = eph_body_radius(eph, i);
        out->flux[slot] = eph_body_flux(eph, i);
        if (out->flux[slot] > 0.0) {
            out->n_emitter++;
        }
    }

    if (out->n_bodies == 0) {
        return CORE_ERR_INVALID_ARG;
    }

    return CORE_OK;
}

void field_clear_harmonics(FieldCtx *ctx)
{
    if (ctx == NULL) {
        return;
    }
    for (int i = 0; i < ctx->n_bodies; i++) {
        ctx->harmonics[i].degree = 0;
    }
    ctx->n_harmonic = 0;
}

void field_set_vessel(FieldCtx *ctx, const VesselParams *vessel)
{
    if (ctx == NULL) {
        return;
    }

    ctx->srp_coeff = 0.0;

    if (vessel == NULL || !(vessel->mass_kg > 0.0) ||
        !(vessel->area_m2 > 0.0) || !(vessel->cr > 0.0)) {
        return;
    }

    ctx->srp_coeff = vessel->cr * vessel->area_m2 / vessel->mass_kg;
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

/* How much of body e's disc is still visible from the vessel, given every
 * body's offset. The darkest occulter wins - see field.h on why that is the
 * right combination and where it stops being exact.
 *
 * Shared by accel_field and field_gradient so that the two cannot disagree
 * about where the shadow is. They would only ever disagree by a rounding
 * step, and a gradient of a slightly different force is exactly the bug K8
 * was about. */
static double emitter_shadow(const FieldCtx *c, const Vec3d *off, int e)
{
    double shadow = 1.0;

    for (int o = 0; o < c->n_bodies; o++) {
        if (o == e) {
            continue;
        }
        double f = srp_shadow(off[e], c->radius[e], off[o], c->radius[o]);
        if (f < shadow) {
            shadow = f;
        }
    }

    return shadow;
}

/* Radiation pressure from every body that shines, summed in index order. */
static Vec3d srp_total(const FieldCtx *c, const Vec3d *off)
{
    Vec3d sum = vec3_zero();

    for (int e = 0; e < c->n_bodies; e++) {
        if (!(c->flux[e] > 0.0)) {
            continue;
        }

        SrpParams p;
        p.flux_1au = c->flux[e];
        p.sun_radius = c->radius[e];
        p.coeff = c->srp_coeff;

        Vec3d term;
        srp_accel(&p, off[e], emitter_shadow(c, off, e), &term);
        sum = vec3_add(sum, term);
    }

    return sum;
}

void accel_field(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out)
{
    (void)v;

    FieldCtx *c = (FieldCtx *)ctx;
    Vec3d sum = vec3_zero();
    Vec3d harmonic = vec3_zero();

    /* Every body's offset, kept rather than consumed: the shadow of one body
     * on another needs both at once, and looking a state up twice would cost
     * a Chebyshev evaluation, which is the expensive part here. */
    Vec3d off[FIELD_MAX_BODIES];

    /* Index order, always. A reordering here is a different trajectory
     * (vec3.h), and this is the sum most likely to be "optimised" later by
     * sorting bodies by distance. */
    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d)) {
            *a_out = vec3_zero();
            return;
        }
        off[i] = d;

        double r2 = vec3_norm_sq(d);
        double rn = sqrt(r2);
        double inv_r3 = 1.0 / (r2 * rn);

        sum = vec3_add_scaled(sum, d, eph_body_mu(c->eph, c->body[i]) * inv_r3);

        /* Computed inside the loop to reuse d rather than look the body's
         * state up a second time - the ephemeris evaluation is the
         * expensive part here, not the arithmetic. Accumulated separately
         * and added once at the end so the point-mass sum keeps exactly the
         * association it had before K4: over an asset of pure point masses,
         * this function is bit-for-bit what it was.
         *
         * harmonics_accel wants the vessel relative to the body, which is
         * -d; the pole is assumed along z, as field.h explains. */
        if (c->harmonics[i].degree >= 2) {
            Vec3d term;
            harmonics_accel(&c->harmonics[i], vec3_neg(d),
                            eph_body_mu(c->eph, c->body[i]), &term);
            harmonic = vec3_add(harmonic, term);
        }
    }

    if (c->n_harmonic > 0) {
        sum = vec3_add(sum, harmonic);
    }

    /* Added last, and skipped entirely for a vessel with no area: an asset
     * carrying a flux does not by itself change any trajectory this file
     * produced before K6b, and that is checkable rather than hoped for. */
    if (c->n_emitter > 0 && c->srp_coeff > 0.0) {
        sum = vec3_add(sum, srp_total(c, off));
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

    Vec3d off[FIELD_MAX_BODIES];

    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d)) {
            for (int k = 0; k < 9; k++) {
                g[k] = 0.0;
            }
            return;
        }
        off[i] = d;

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

        /* And the body's shape, where it has one (ROADMAP K8b). No sign to
         * get wrong here even though the point-mass block above has one:
         * that one differentiates through d = R - r, while
         * harmonics_gradient is already a derivative with respect to the
         * vessel's own position relative to the body, which is what
         * accel_field passes it.
         *
         * Upper triangle only, matching the block above - the matrix comes
         * back symmetric to the bit, so taking its lower half would add the
         * same numbers in a different order. */
        if (c->harmonics[i].degree >= 2) {
            double hg[9];
            harmonics_gradient(&c->harmonics[i], vec3_neg(d), mu, hg);

            g[0] += hg[0];
            g[4] += hg[4];
            g[8] += hg[8];

            g[1] += hg[1];
            g[2] += hg[2];
            g[5] += hg[5];
        }
    }

    /* And the smooth part of radiation pressure, on the same terms as the
     * force itself: only for a vessel that has an area, and with the shadow
     * held where accel_field put it (core/srp.h). Whole matrix rather than
     * the upper triangle, because srp_gradient already returns a symmetric
     * one and mirroring below would then copy over a term already added. */
    if (c->n_emitter > 0 && c->srp_coeff > 0.0) {
        for (int e = 0; e < c->n_bodies; e++) {
            if (!(c->flux[e] > 0.0)) {
                continue;
            }

            SrpParams p;
            p.flux_1au = c->flux[e];
            p.sun_radius = c->radius[e];
            p.coeff = c->srp_coeff;

            double sg[9];
            srp_gradient(&p, off[e], emitter_shadow(c, off, e), sg);

            g[0] += sg[0];
            g[4] += sg[4];
            g[8] += sg[8];

            g[1] += sg[1];
            g[2] += sg[2];
            g[5] += sg[5];
        }
    }

    g[3] = g[1];
    g[6] = g[2];
    g[7] = g[5];
}

void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out)
{
    FieldCtx *c = (FieldCtx *)ctx;

    accel_field(t, r[0], v[0], ctx, &a_out[0]);

    if (n_blocks < 2) {
        return;
    }

    double g[9];
    field_gradient(t, r[0], c, g);

    for (int b = 1; b < n_blocks; b++) {
        Vec3d dr = r[b];

        a_out[b].x = g[0] * dr.x + g[1] * dr.y + g[2] * dr.z;
        a_out[b].y = g[3] * dr.x + g[4] * dr.y + g[5] * dr.z;
        a_out[b].z = g[6] * dr.x + g[7] * dr.y + g[8] * dr.z;
    }
}
