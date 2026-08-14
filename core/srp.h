/* Solar radiation pressure with a conical shadow (ROADMAP K6).
 *
 * PROJECT.md section 4 lists SRP among the four effects a point mass cannot
 * produce, and section 8 says why it is the one that matters most: SRP is the
 * dominant error source in real interplanetary navigation, which is what gives
 * the uncertainty mechanic a physical basis instead of an invented one. A
 * covariance that grows because the force model is genuinely uncertain is a
 * different thing from a covariance that grows because the designer said so.
 *
 * This file is the isolated math, in the same split K1 used before K2 and K3a
 * used before K3b: geometry and photons here, bodies and the asset in K6b
 * (core/field.h). Nothing here knows what an ephemeris is.
 *
 * ---
 *
 * TWO THINGS THE ROADMAP ENTRY FOR K6 GOT WRONG, worth stating because they
 * changed the shape of the step:
 *
 * 1. "Depends only on positions" is not true. A conical shadow needs the
 *    radius of the Sun and of the occulting body, and the asset carried
 *    neither - core/prop.h says so explicitly where it explains why
 *    CORE_EVENT_DISTANCE measures distance from a centre rather than
 *    altitude. Radii ship in asset version 3 with K6b, which is also what
 *    K7 needed and what turns that event into an altitude event.
 *
 * 2. A massless test particle feels no radiation pressure. The acceleration
 *    scales with Cr*A/m, which is a property of the vessel and of nothing
 *    else, so K6 is where VesselParams finally arrives - the struct
 *    core/prop.h deliberately refused to add while every field of it would
 *    have been ignored.
 *
 * ---
 *
 * NO TRIGONOMETRY, and here that took work rather than care.
 *
 * The fraction of the solar disc an occulting body covers is the overlap area
 * of two circles, and the classical formula for that is written in arc
 * cosines. CLAUDE.md invariant 3 allows +, -, *, / and sqrt and nothing else,
 * because libm's acos is not guaranteed bit-identical between platforms or
 * even between libc versions.
 *
 * So srp_acos below is our own polynomial approximation, whose error is
 * measured in core/test/test_srp.c against libm rather than quoted. That is
 * the honest version of "algebraic approximation of the cone" the ROADMAP
 * asked for: not a smoothstep across the penumbra chosen because it is
 * smooth, but the real geometry evaluated with an approximation whose error
 * is a number we know. */

#ifndef CORE_SRP_H
#define CORE_SRP_H

#include "vec3.h"

/* Both are definitions rather than measurements, so no data file cites them
 * and none needs to: the astronomical unit has been exactly this many metres
 * since IAU 2012 Resolution B2, and the speed of light exactly this many
 * metres per second since the 1983 redefinition of the metre. The one number
 * here that IS a measurement - the solar flux - travels in SrpParams from the
 * asset, cited where the cooker reads it. */
#define SRP_AU_M  1.495978707e11
#define SRP_C_M_S 299792458.0

typedef struct {
    /* Solar irradiance at one astronomical unit, W/m^2. */
    double flux_1au;

    /* Radius of the emitting body, metres. Only the shadow geometry uses it;
     * the pressure itself is a point source at this distance. */
    double sun_radius;

    /* The vessel's reflectivity coefficient times its area over its mass,
     * m^2/kg. One number rather than three because the acceleration only ever
     * depends on the product, and carrying Cr, A and m separately would let a
     * caller believe the model distinguishes them.
     *
     * Cr is 1 for a perfect absorber and 2 for a perfect specular reflector;
     * real spacecraft sit near 1.3, and it is uncertain by several per cent,
     * which is precisely the uncertainty PROJECT.md section 8 wants to make
     * playable. */
    double coeff;
} SrpParams;

/* Arc cosine on [-1, 1], to about 2.2e-8 radians, using only the four
 * permitted operations. Abramowitz and Stegun 4.4.46 for the positive half,
 * reflected through acos(-x) = pi - acos(x) for the negative one.
 *
 * Public because an approximation whose error nobody measures is a guess.
 * core/test/test_srp.c sweeps it against libm's acos and fails if the error
 * moves; that test may link libm, the runtime may not.
 *
 * It lives here rather than in a general "math without libm" header on
 * purpose. A general one invites callers whose accuracy needs nobody has
 * checked, and 2.2e-8 radians is a fine number for a penumbra and a poor one
 * for, say, a rotation matrix. */
double srp_acos(double x);

/* Fraction of the Sun's disc still visible from the vessel, 0 (full umbra)
 * to 1 (unobstructed), given the vessel-to-Sun and vessel-to-body vectors and
 * the two radii.
 *
 * Both discs are treated as circles on the sky whose angular radii satisfy
 * sin(theta) = R / d - which is exact for a sphere, not a small-angle
 * approximation - and the overlap is the planar two-circle area. That last
 * step is the model's one real approximation (Montenbruck and Gill section
 * 3.4 make the same one) and it is good wherever both discs are small, which
 * is everywhere except skimming a body's surface.
 *
 * Returns 1 whenever the body cannot occult: zero or unknown radius, or the
 * body sitting on the far side of the vessel from the Sun. Returns 0 for a
 * vessel inside the body, which is nonsense geometry but the answer least
 * likely to be mistaken for a working orbit. */
double srp_shadow(Vec3d to_sun, double sun_radius,
                  Vec3d to_body, double body_radius);

/* Acceleration from radiation pressure: away from the Sun, falling off as
 * 1/d^2, scaled by the shadow fraction.
 *
 *     a = -(flux_1au * (AU/d)^2 / c) * coeff * shadow * to_sun / d
 *
 * Absorption only. A real spacecraft also feels a component along its own
 * normal from specular reflection and one from re-radiated heat, and both are
 * folded into Cr here, which is what Cr is for. */
void srp_accel(const SrpParams *p, Vec3d to_sun, double shadow, Vec3d *a_out);

/* Gradient of that acceleration with respect to the vessel's position,
 * row-major 3x3 and symmetric - the piece a state transition matrix needs
 * (ROADMAP K8 taught this lesson: an STM that linearises a force model
 * different from the one being flown is worse than no STM).
 *
 * THE SHADOW FRACTION IS HELD CONSTANT HERE, and that omission is deliberate
 * and measured rather than forgotten. Its true derivative is zero everywhere
 * except inside the penumbra, where it is a spike: the fraction runs the whole
 * way from 1 to 0 across a few tens of kilometres of a low orbit. A spike is
 * not something a linearisation over a leg of hours can carry, and including
 * it would make the matrix depend on whether an integrator step happened to
 * land in the penumbra.
 *
 * The measurement, from core/test/test_srp.c at 7000 km from the Earth's
 * centre with Cr*A/m = 0.02, is not the flattering one and is worth stating
 * plainly. The dropped term peaks at 1.78e-12 s^-2, six orders of magnitude
 * LARGER than the smooth 1/d^2 term this function does keep (2.65e-18). What
 * settles it is the third number: the Earth's own point-mass gradient there
 * is 2.32e-6 s^-2, so even the spike is under a millionth of what the matrix
 * is mostly made of, and it exists only for the seconds a vessel spends
 * crossing a penumbra. The smooth term is kept anyway, at no cost, so that
 * field_gradient stays the exact derivative of accel_field rather than the
 * derivative of a nearby field - the lesson of ROADMAP K8. */
void srp_gradient(const SrpParams *p, Vec3d to_sun, double shadow,
                  double g_out[9]);

#endif /* CORE_SRP_H */
