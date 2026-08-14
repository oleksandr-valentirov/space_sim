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
 * Point masses were all of it once. Harmonics arrived with K4b and radiation
 * pressure with K6b, and both went in the way this file expected them to: the
 * same AccelFunc, the same context, more terms inside. Drag is what is left
 * (K7), and it will be the first of them to read the velocity argument.
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
#include "srp.h"
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

    /* Each body's size and brightness, read from the asset alongside the
     * harmonics (ROADMAP K6b). Radius 0 means the asset does not say how big
     * the body is, and such a body occults nothing; flux 0 means it does not
     * shine, which is every body but one.
     *
     * There is no "which body is the Sun" here on purpose. A vessel feels
     * radiation pressure from every body with a positive flux, summed in
     * index order like everything else, and shadowed by every other body.
     * With one star that is one term - and no identity to get wrong. */
    double radius[FIELD_MAX_BODIES];
    double flux[FIELD_MAX_BODIES];
    int    n_emitter;    /* how many have flux > 0, for the fast path */

    /* Each body's air, read from the asset alongside everything else
     * (ROADMAP K7b). No layers means airless, which is nine of the fixture's
     * ten bodies. Altitudes inside are above radius[] - the mean radius, not
     * the harmonics' reference radius, which for the Earth is a different
     * number. */
    AtmosphereModel atmosphere[FIELD_MAX_BODIES];
    int             n_atmosphere;   /* how many have layers, for the fast path */

    /* The vessel's Cr * A / m and Cd * A / m, m^2/kg. Zero - which is what a
     * caller that never asks for either gets - means that term is not
     * evaluated at all, and the trajectory is bit-for-bit the one this file
     * produced before K6b and K7b respectively. Both set through
     * field_set_vessel.
     *
     * Unlike the harmonics above, these HAVE a setter, and the difference is
     * not an inconsistency. Coefficients describe the Earth, so letting a
     * caller supply them would let two callers fly past two different Earths
     * (K4b). Cr*A/m and Cd*A/m describe the spacecraft, and two spacecraft
     * differing in them is the entire point. */
    double srp_coeff;
    double drag_coeff;

    /* Every density this context produces is multiplied by this (ROADMAP
     * K7b). One is the plain USSA-76 profile and is what a zeroed context
     * carries, so it changes nothing until somebody asks.
     *
     * It exists because the atmosphere above 300 km swings by more than an
     * order of magnitude over the solar cycle, and the table is one static
     * profile. What it deliberately is NOT is a function of time evaluated
     * here: a solar cycle is a sine with an eleven-year period, libm is
     * forbidden in this loop, and nobody can say where the next maximum
     * falls anyway. So the game computes it outside and it stays constant
     * across a leg, exactly as VesselParams does. */
    double density_scale;

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

/* Give the field a vessel to push on (ROADMAP K6b, K7b).
 *
 * Only the products Cr * A / m and Cd * A / m are kept, because only the
 * products are ever used; see core/srp.h and core/atmosphere.h. A NULL
 * vessel, a non-positive mass or a zero area mean neither force; a zero cr or
 * cd switches off just that one.
 *
 * Set per run rather than per context: mass changes when fuel burns, and the
 * one propagator the game owns is shared by every vessel it flies
 * (core/prop.h). */
void field_set_vessel(FieldCtx *ctx, const VesselParams *vessel);

/* Scale every density this context reports (ROADMAP K7b). One restores the
 * asset's own profile; a non-positive value is ignored, since a zero or
 * negative atmosphere is not a thinner one.
 *
 * Separate from field_set_vessel because it describes the environment rather
 * than the spacecraft: two vessels in the same leg fly through the same air.
 * See the struct field for why it is a constant and not a function of time. */
void field_set_density_scale(FieldCtx *ctx, double scale);

/* Sum of mu_i (R_i - r) / |R_i - r|^3 over the context's bodies, in index
 * order, plus each body's harmonic term where the asset gives it one, plus
 * radiation pressure and drag where the vessel and the asset allow them.
 * Matches AccelFunc.
 *
 * THE VELOCITY ARGUMENT IS READ SINCE K7b. It was ignored from C4 until then,
 * because gravity in an inertial frame has no velocity dependence and neither
 * does sunlight; drag has, and that dependence is why PROJECT.md section 4
 * chose DOP853 over RKN in the first place.
 *
 * FRAME: harmonics are applied with each body's pole assumed to lie along
 * the ephemeris frame's own z axis, exactly as the cooker does (see
 * core/offline/nbody.c) and for the same reason - K3a measured what that
 * costs and it is under 1% of the Moon's error. For a zonal field that is
 * the whole story, since only the pole enters; the prime meridian, whose
 * error is much larger (core/offline/body_rotation.h), cancels. A tesseral
 * field is a different matter and waits for K5.
 *
 * Drag does NOT make that assumption, and is the first thing here that does
 * not: the air co-rotates with the body, so the wind comes from the body's
 * real angular velocity (eph_body_angular_velocity), read from the K3b
 * orientation channels. The atmosphere itself is spherical, so only the wind
 * needs the rotation - altitude does not. */
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

/* Both gradients of the acceleration, row-major 3x3. dadv may be NULL when a
 * caller wants only the position one. Public because these are the pieces
 * worth testing on their own.
 *
 * Includes each body's harmonic term where the asset gives it one, the smooth
 * part of the radiation pressure term, and drag, so this stays the exact
 * derivative of accel_field rather than the derivative of a simpler field
 * that happens to share its name.
 *
 * "Smooth part" is the one honest gap and core/srp.h holds its measurement:
 * the shadow fraction is treated as locally constant, because its true
 * derivative is a spike confined to the penumbra and worth a millionth of
 * the gravity gradient beside it.
 *
 * dadr IS SYMMETRIC ONLY WHILE DRAG IS OFF, and that is a narrowing of what
 * this function used to promise. Gravity's gradient is a Hessian and
 * symmetric by construction; sunlight's is too. Drag's is not: position
 * reaches it through the density, so it carries a rank-one outer product of
 * the relative velocity with the local vertical, plus the wind term below.
 * A caller that mirrors half of this matrix onto the other half was correct
 * until K7b and is not now.
 *
 * dadv is entirely new. Before drag every force here was velocity-independent
 * and accel_field_var could compute a perturbation's acceleration from its
 * position offset alone; it now needs both.
 *
 * THE WIND IS DIFFERENTIATED TOO. Because the air co-rotates, v_rel depends
 * on where the vessel is, so the full position gradient is the density term
 * plus dadv * (-[omega]x). Leaving it out would be the K8 mistake again: an
 * STM that linearises a force model slightly different from the one being
 * flown. */
void field_gradient(double t, Vec3d r, Vec3d v, const FieldCtx *ctx,
                    double dadr[9], double dadv[9]);

#endif /* CORE_FIELD_H */
