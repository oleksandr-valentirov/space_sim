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

    /* Each body's shape, read from the asset when the context is built
     * (ROADMAP K4b) and indexed alongside body[]. Degree 0 means point
     * mass, which is what every body but the Earth is today.
     *
     * Read rather than supplied: these are the same coefficients the
     * cooker integrated the bodies under (core/ephemeris.h), so a vessel
     * and the bodies cannot disagree about the shape of the Earth. There
     * is deliberately no setter - the version of this that took one let a
     * caller pass numbers unrelated to the asset, and nothing would have
     * said so.
     *
     * Named for harmonics rather than for J2, unlike NBodySystem's has_j2:
     * the cooker only needs the low degrees that move one body under
     * another (PROJECT.md section 4), while this is the field a vessel
     * flies in, and K5 puts a lunar model here. */
    HarmonicsField harmonics[FIELD_MAX_BODIES];
    int            n_harmonic;   /* how many have degree >= 2, for the fast path */

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

/* Drop every harmonic term, leaving point masses.
 *
 * Exists for measurement, not for configuration: it is how a test says
 * "the same asset, the same bodies, without this one effect" and gets a
 * number to compare against. Flying with it is flying a field the asset
 * does not describe. */
void field_clear_harmonics(FieldCtx *ctx);

/* Sum of mu_i (R_i - r) / |R_i - r|^3 over the context's bodies, in index
 * order, plus each body's harmonic term where the asset gives it one.
 * Matches AccelFunc; velocity is unused and will not be once drag exists.
 *
 * FRAME: harmonics are applied with each body's pole assumed to lie along
 * the ephemeris frame's own z axis, exactly as the cooker does (see
 * core/offline/nbody.c) and for the same reason - body orientation is
 * K3b, and K3a measured what its absence costs. For a zonal field that is
 * the whole story, since only the pole enters; the prime meridian, whose
 * error is much larger (core/offline/body_rotation.h), cancels. A tesseral
 * field is a different matter and must wait for K3b. */
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
 * Harmonics included since ROADMAP K8b. Between K4 and K8a this refused
 * outright rather than linearise a field it could only half describe -
 * the Hessian of the Pines recursion did not exist yet, and a matrix that
 * looks like a state transition matrix while matching some other
 * trajectory is worse than no matrix. harmonics_gradient is that missing
 * piece; the refusal is gone because the reason for it is. */
void accel_field_var(double t, const Vec3d *r, const Vec3d *v, int n_blocks,
                     void *ctx, Vec3d *a_out);

/* Gradient of the acceleration at r, row-major 3x3 and symmetric. Public
 * because it is the piece worth testing on its own.
 *
 * Includes each body's harmonic term where the asset gives it one, so
 * this stays the exact derivative of accel_field rather than the
 * derivative of a simpler field that happens to share its name. */
void field_gradient(double t, Vec3d r, const FieldCtx *ctx, double g[9]);

#endif /* CORE_FIELD_H */
