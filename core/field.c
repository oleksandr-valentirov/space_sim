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

        /* And the air (ROADMAP K7b), once, here. */
        if (eph_body_atmosphere(eph, i, &out->atmosphere[slot]) != CORE_OK) {
            return CORE_ERR_INVALID_ARG;
        }

        /* An atmosphere without a radius has no altitude to stand on: every
         * layer in the asset is measured above the body's mean radius, and
         * without one a vessel a thousand kilometres up would be told it is
         * a thousand kilometres deep. Dropped rather than flown, since the
         * asset saying one and not the other is a cooker bug, and a
         * plausible wrong drag is worse than none. */
        if (out->atmosphere[slot].n_layers > 0 && !(out->radius[slot] > 0.0)) {
            out->atmosphere[slot].n_layers = 0;
        }
        if (out->atmosphere[slot].n_layers > 0) {
            out->n_atmosphere++;
        }
    }

    if (out->n_bodies == 0) {
        return CORE_ERR_INVALID_ARG;
    }

    /* memset above zeroed it, and zero is not the neutral value here. */
    out->density_scale = 1.0;

    return CORE_OK;
}

void field_exclude(FieldCtx *ctx, int body)
{
    if (ctx == NULL) {
        return;
    }

    int out = 0;
    for (int i = 0; i < ctx->n_bodies; i++) {
        if (ctx->body[i] == body) {
            continue;
        }
        if (out != i) {
            ctx->body[out] = ctx->body[i];
            ctx->harmonics[out] = ctx->harmonics[i];
            ctx->radius[out] = ctx->radius[i];
            ctx->flux[out] = ctx->flux[i];
            ctx->atmosphere[out] = ctx->atmosphere[i];
        }
        out++;
    }
    ctx->n_bodies = out;

    /* Recounted rather than decremented, so that adding a fifth per-body
     * array later means adding it to the loop above and nothing else. */
    ctx->n_harmonic = 0;
    ctx->n_emitter = 0;
    ctx->n_atmosphere = 0;
    for (int i = 0; i < ctx->n_bodies; i++) {
        if (ctx->harmonics[i].degree >= 2) {
            ctx->n_harmonic++;
        }
        if (ctx->flux[i] > 0.0) {
            ctx->n_emitter++;
        }
        if (ctx->atmosphere[i].n_layers > 0) {
            ctx->n_atmosphere++;
        }
    }
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
    ctx->drag_coeff = 0.0;

    if (vessel == NULL || !(vessel->mass_kg > 0.0) ||
        !(vessel->area_m2 > 0.0)) {
        return;
    }

    /* Two coefficients from one area, and each switched off by its own
     * coefficient being zero: a vessel given a cr and no cd feels sunlight
     * and no air, which is exactly what every caller written before K7b
     * passes. */
    if (vessel->cr > 0.0) {
        ctx->srp_coeff = vessel->cr * vessel->area_m2 / vessel->mass_kg;
    }
    if (vessel->cd > 0.0) {
        ctx->drag_coeff = vessel->cd * vessel->area_m2 / vessel->mass_kg;
    }
}

void field_set_density_scale(FieldCtx *ctx, double scale)
{
    if (ctx == NULL || !(scale > 0.0)) {
        return;
    }
    ctx->density_scale = scale;
}

/* Position of body index b at t, and the vector from r to it. The body's own
 * velocity goes to vel_out when that is not NULL - drag needs it, nothing
 * else does, and the ephemeris hands both over together anyway. Returns 0 and
 * trips the failure flag if the ephemeris cannot answer. */
static int offset_to_body(FieldCtx *c, int b, double t, Vec3d r, Vec3d *out,
                          Vec3d *vel_out)
{
    State s;
    if (eph_body_state(c->eph, c->body[b], t, &s) != CORE_OK) {
        c->failed = 1;
        return 0;
    }

    *out = vec3_sub(s.r, r);
    if (vel_out != NULL) {
        *vel_out = s.v;
    }
    return 1;
}

/* Everything the drag on one body depends on, gathered in one place.
 *
 * Shared by accel_field and field_gradient for the same reason emitter_shadow
 * is: two evaluations of "how fast is the air going past" that differ by a
 * rounding step would make the gradient the derivative of a slightly
 * different force, which is the bug ROADMAP K8 was about.
 *
 * off is the vector from the vessel TO the body, as the rest of this file
 * uses; the vessel relative to the body is its negation. */
typedef struct {
    Vec3d  v_rel;     /* vessel velocity relative to the co-rotating air */
    Vec3d  up;        /* unit vector along increasing altitude */
    Vec3d  omega;     /* the body's angular velocity, ephemeris frame */
    double rho;       /* scaled density */
    double drho_dh;   /* and its vertical derivative, scaled the same */
} DragTerms;

static int drag_terms(FieldCtx *c, int i, double t, Vec3d off, Vec3d v,
                      Vec3d body_v, DragTerms *out)
{
    Vec3d rel = vec3_neg(off);
    double dist = vec3_norm(rel);
    if (!(dist > 0.0)) {
        return 0;   /* inside the body's centre; no vertical to speak of */
    }

    atmosphere_density(&c->atmosphere[i], dist - c->radius[i],
                       &out->rho, &out->drho_dh);
    out->rho *= c->density_scale;
    out->drho_dh *= c->density_scale;

    if (!(out->rho > 0.0)) {
        return 0;   /* above the air, which is where a vessel usually is */
    }

    /* Read after the density, so a vessel in vacuum never pays for it. The
     * time is one the ephemeris has already answered for in this call, so a
     * failure here is a corrupt asset rather than a range error - reported
     * the same way regardless. */
    if (eph_body_angular_velocity(c->eph, c->body[i], t, &out->omega)
            != CORE_OK) {
        c->failed = 1;
        return 0;
    }

    /* The air moves with the body: its orbital velocity plus the rotation at
     * the vessel's position. Both subtracted from the vessel's inertial
     * velocity, in that order. */
    Vec3d wind = vec3_cross(out->omega, rel);
    out->v_rel = vec3_sub(vec3_sub(v, body_v), wind);

    out->up = vec3_scale(rel, 1.0 / dist);
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
    FieldCtx *c = (FieldCtx *)ctx;
    Vec3d sum = vec3_zero();
    Vec3d harmonic = vec3_zero();

    /* Every body's offset, kept rather than consumed: the shadow of one body
     * on another needs both at once, and looking a state up twice would cost
     * a Chebyshev evaluation, which is the expensive part here. */
    Vec3d off[FIELD_MAX_BODIES];
    Vec3d bvel[FIELD_MAX_BODIES];

    int want_drag = c->n_atmosphere > 0 && c->drag_coeff > 0.0;

    /* Index order, always. A reordering here is a different trajectory
     * (vec3.h), and this is the sum most likely to be "optimised" later by
     * sorting bodies by distance. */
    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d, want_drag ? &bvel[i] : NULL)) {
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

    /* And the air, last and on the same terms (ROADMAP K7b): an asset
     * carrying an atmosphere changes no trajectory this file produced before
     * it, for any vessel that does not ask for drag. */
    if (want_drag) {
        for (int i = 0; i < c->n_bodies; i++) {
            if (c->atmosphere[i].n_layers == 0) {
                continue;
            }

            DragTerms dt;
            if (!drag_terms(c, i, t, off[i], v, bvel[i], &dt)) {
                continue;
            }

            Vec3d term;
            drag_accel(dt.rho, c->drag_coeff, dt.v_rel, &term);
            sum = vec3_add(sum, term);
        }
    }

    *a_out = sum;
}

void field_gradient(double t, Vec3d r, Vec3d v, const FieldCtx *ctx,
                    double g[9], double gv[9])
{
    /* Const in the signature because a gradient is a measurement, not a step;
     * the cast is to reuse offset_to_body, and the only state it touches is
     * the failure flag, which a failed gradient should set too. */
    FieldCtx *c = (FieldCtx *)ctx;

    for (int k = 0; k < 9; k++) {
        g[k] = 0.0;
        if (gv != NULL) {
            gv[k] = 0.0;
        }
    }

    Vec3d off[FIELD_MAX_BODIES];
    Vec3d bvel[FIELD_MAX_BODIES];

    int want_drag = c->n_atmosphere > 0 && c->drag_coeff > 0.0;

    for (int i = 0; i < c->n_bodies; i++) {
        Vec3d d;
        if (!offset_to_body(c, i, t, r, &d, want_drag ? &bvel[i] : NULL)) {
            for (int k = 0; k < 9; k++) {
                g[k] = 0.0;
                if (gv != NULL) {
                    gv[k] = 0.0;
                }
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

    /* Everything above this line is symmetric by construction, so only the
     * upper triangle was accumulated. Mirror it before drag, which is not. */
    g[3] = g[1];
    g[6] = g[2];
    g[7] = g[5];

    if (!want_drag) {
        return;
    }

    for (int i = 0; i < c->n_bodies; i++) {
        if (c->atmosphere[i].n_layers == 0) {
            continue;
        }

        DragTerms dt;
        if (!drag_terms(c, i, t, off[i], v, bvel[i], &dt)) {
            continue;
        }

        double dr[9], dv[9];
        drag_jacobian(dt.rho, dt.drho_dh, c->drag_coeff, dt.v_rel, dt.up,
                      dr, dv);

        /* The wind term. v_rel = v - v_body - omega x rel and rel = r - R, so
         * d(v_rel)/d(r) = -[omega]x, and the chain rule adds
         *
         *     d(a)/d(v_rel) * d(v_rel)/d(r) = dv * (-[omega]x)
         *
         * to the density term drag_jacobian already returned. Without it this
         * would be the derivative of drag through a still atmosphere, which
         * is not the force being flown - the K8 mistake exactly.
         *
         * [omega]x u = omega x u, so its columns are omega x e_j:
         *
         *     [  0  -wz   wy ]
         *     [ wz    0  -wx ]
         *     [-wy   wx    0 ]
         *
         * and the product is written out rather than looped, because a
         * three-by-three matrix multiply written as a loop over a
         * skew-symmetric operand is four lines of index arithmetic nobody
         * can check by eye. */
        double wx = dt.omega.x, wy = dt.omega.y, wz = dt.omega.z;

        for (int row = 0; row < 3; row++) {
            double m0 = dv[row * 3 + 0];
            double m1 = dv[row * 3 + 1];
            double m2 = dv[row * 3 + 2];

            /* -(row of dv) * [omega]x, column by column. */
            g[row * 3 + 0] += dr[row * 3 + 0] - (m1 * wz - m2 * wy);
            g[row * 3 + 1] += dr[row * 3 + 1] - (m2 * wx - m0 * wz);
            g[row * 3 + 2] += dr[row * 3 + 2] - (m0 * wy - m1 * wx);
        }

        if (gv != NULL) {
            for (int k = 0; k < 9; k++) {
                gv[k] += dv[k];
            }
        }
    }
}

void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out)
{
    FieldCtx *c = (FieldCtx *)ctx;

    accel_field(t, r[0], v[0], ctx, &a_out[0]);

    if (n_blocks < 2) {
        return;
    }

    double g[9], gv[9];
    field_gradient(t, r[0], v[0], c, g, gv);

    /* A perturbation's acceleration is the Jacobian applied to the
     * perturbation, and until K7b the velocity half of that Jacobian was
     * identically zero - gravity in an inertial frame has no velocity
     * dependence and neither does sunlight - so this loop read r[b] and
     * ignored v[b]. Drag has one, and a linearisation that kept ignoring it
     * would describe a vessel whose air resistance does not care how fast it
     * is going. */
    for (int b = 1; b < n_blocks; b++) {
        Vec3d dr = r[b];
        Vec3d dv = v[b];

        a_out[b].x = g[0] * dr.x + g[1] * dr.y + g[2] * dr.z
                   + gv[0] * dv.x + gv[1] * dv.y + gv[2] * dv.z;
        a_out[b].y = g[3] * dr.x + g[4] * dr.y + g[5] * dr.z
                   + gv[3] * dv.x + gv[4] * dv.y + gv[5] * dv.z;
        a_out[b].z = g[6] * dr.x + g[7] * dr.y + g[8] * dr.z
                   + gv[6] * dv.x + gv[7] * dv.y + gv[8] * dv.z;
    }
}
