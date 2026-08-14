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
#include "harmonics.h"
#include "vec3.h"

#define FIELD_MAX_BODIES 16

typedef struct {
    const EphemerisCtx *eph;

    int n_bodies;
    int body[FIELD_MAX_BODIES];   /* indices into the ephemeris */

    /* Oblateness of at most one of those bodies (ROADMAP K4), set through
     * field_set_harmonics and off in any zero-initialised context - so a
     * caller that never asks for it gets bit-for-bit what this file
     * computed before K4 existed.
     *
     * Named for harmonics rather than for J2, unlike NBodySystem's has_j2:
     * the cooker only ever needs the low degrees that move one body under
     * another (PROJECT.md section 4), while this is the field a vessel
     * flies in, and K5 puts a degree-50 GRAIL lunar model right here.
     *
     * harmonics_slot indexes body[], not the ephemeris - resolved once when
     * it is set, so the force loop does not search for it. */
    int            has_harmonics;
    int            harmonics_slot;
    HarmonicsField harmonics;

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

/* Give one of the context's bodies a gravity field beyond a point mass
 * (ROADMAP K4). `body` is an ephemeris index, as passed to field_all_but,
 * and must be one this context actually sums over - asking for the body
 * that field_all_but excluded is an error rather than a no-op, because it
 * would otherwise read as "harmonics enabled" while nothing applied them.
 *
 * The coefficients are supplied by the caller, not read from the asset:
 * the ephemeris format has no place for them yet. See core/cook/ for where
 * the same numbers are cited on the cooker side.
 *
 * FRAME: the field is applied with the body's pole assumed to lie along
 * the ephemeris frame's own z axis, exactly as the cooker does (see
 * core/offline/nbody.c) and for the same reason - body orientation is
 * K3b, and K3a measured what its absence costs. For a zonal field that is
 * the whole story, since only the pole enters; the prime meridian, whose
 * error is much larger (core/offline/body_rotation.h), cancels. A tesseral
 * field is a different matter and must wait for K3b. */
CoreResult field_set_harmonics(FieldCtx *ctx, int body,
                               const HarmonicsField *field);

/* Sum of mu_i (R_i - r) / |R_i - r|^3 over the context's bodies, in index
 * order, plus the harmonic term of field_set_harmonics if one is set.
 * Matches AccelFunc; velocity is unused and will not be once drag
 * exists. */
void accel_field(double t, Vec3d r, Vec3d v, void *ctx, Vec3d *a_out);

/* The same field plus its linearisation, for stm_integrate. Block 0 is the
 * vessel; blocks 1..n-1 are perturbations about it, each accelerated by
 *
 *     sum_i mu_i [ 3 d_i d_i^T / |d_i|^5 - I / |d_i|^3 ] dr,   d_i = R_i - r
 *
 * with no velocity term, since point-mass gravity in an inertial frame has
 * none. That absence is worth stating, because the CR3BP version does have
 * one and the two are easy to confuse.
 *
 * POINT MASSES ONLY, and it refuses rather than approximates: given a
 * context with harmonics set, this sets `failed` and writes zeros instead
 * of linearising only part of the force it was asked about. The reason is
 * the one in the header comment above - an answer that looks like a state
 * transition matrix and does not match the trajectory actually propagated
 * is worse than no answer, and this file already takes that position about
 * a failed ephemeris lookup.
 *
 * The missing piece is the Hessian of the Pines recursion, which is real
 * work with its own tests rather than a line to add here; it belongs with
 * prop_run_stm in ROADMAP K8. Until then a caller wanting an STM uses a
 * context without harmonics, and gets an STM that honestly describes that
 * field. */
void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out);

/* Gradient of the acceleration at r, row-major 3x3 and symmetric. Public
 * because it is the piece worth testing on its own.
 *
 * Point masses only, refusing on a harmonic context exactly as
 * accel_field_var does - it is the function accel_field_var refuses
 * through. */
void field_gradient(double t, Vec3d r, const FieldCtx *ctx, double g[9]);

#endif /* CORE_FIELD_H */
