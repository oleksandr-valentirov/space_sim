/* The gravity field of the ephemeris, as felt by a vessel (ROADMAP C4).
 *
 * Everything up to now has propagated either a toy problem or the bodies
 * themselves. This is the first force model a vessel actually flies in: point
 * masses read from the cooked asset, summed in a fixed order, in the same
 * inertial barycentric frame the asset uses.
 *
 * A vessel is massless here and that is not an approximation to be improved
 * later - it is the split the whole architecture rests on. Bodies move on
 * rails computed once by the cooker (PROJECT.md section 4); vessels are test
 * particles in the field those rails describe. Making a vessel pull on a
 * planet would mean the ephemeris depended on the save file.
 *
 * Point masses only for now. Harmonics, SRP and drag arrive at M3.5, and the
 * shape of this interface is what they will extend: the same AccelFunc, the
 * same context, more terms inside.
 *
 * ---
 *
 * Errors from a void callback. AccelFunc cannot return a result code - it is
 * called from inside an integrator stage, which has nowhere to put one - and
 * the ephemeris genuinely can fail, by being asked for a time outside its
 * span. Returning zero acceleration there would produce a trajectory that
 * looks plausible and is wrong, which is the worst available outcome.
 *
 * So the context carries a sticky error flag. Zero it before a run, check it
 * after: field_failed() is true if any evaluation could not be made. The
 * acceleration returned in that case is zero, so the integrator finishes
 * rather than producing NaN, but the answer is not to be used. */

#ifndef CORE_FIELD_H
#define CORE_FIELD_H

#include "ephemeris.h"
#include "vec3.h"

#define FIELD_MAX_BODIES 16

typedef struct {
    const EphemerisCtx *eph;

    int n_bodies;
    int body[FIELD_MAX_BODIES];   /* indices into the ephemeris */

    /* Sticky: set on the first failed evaluation, never cleared except by the
     * caller. Mutable through a const-free context pointer, which is why the
     * accel functions take void* rather than a const one. */
    int failed;
} FieldCtx;

/* Every body in the ephemeris, in the asset's own order. */
CoreResult field_all_bodies(const EphemerisCtx *eph, FieldCtx *out);

/* Every body except one. Exists for the test that gives this file its oracle:
 * a massless particle placed on a body's own state, flying in the field of
 * all the others, must track that body - because that is precisely the
 * acceleration the cooker integrated it under. */
CoreResult field_all_but(const EphemerisCtx *eph, int excluded, FieldCtx *out);

/* Sum of mu_i (R_i - r) / |R_i - r|^3 over the context's bodies, in index
 * order. Matches AccelFunc; velocity is unused and will not be once drag
 * exists. */
void accel_field(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out);

/* The same field plus its linearisation, for stm_integrate. Block 0 is the
 * vessel; blocks 1..n-1 are perturbations about it, each accelerated by
 *
 *     sum_i mu_i [ 3 d_i d_i^T / |d_i|^5 - I / |d_i|^3 ] dr,   d_i = R_i - r
 *
 * with no velocity term, since point-mass gravity in an inertial frame has
 * none. That absence is worth stating, because the CR3BP version does have
 * one and the two are easy to confuse. */
void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out);

/* Gradient of the acceleration at r, row-major 3x3 and symmetric. Public
 * because it is the piece worth testing on its own. */
void field_gradient(double t, Vec3d r, const FieldCtx *ctx, double g[9]);

#endif /* CORE_FIELD_H */
