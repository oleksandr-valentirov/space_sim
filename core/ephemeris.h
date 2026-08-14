/* Reading the ephemeris asset (ROADMAP B6).
 *
 * This is the runtime side: the asset is built once by the cooker on the
 * developer's machine (core/offline/eph_build.h) and read here on every force
 * evaluation. Nothing in this path uses libm beyond sqrt, and nothing parses
 * text - decimal to double conversion is not guaranteed identical across C
 * libraries, so it stays in the cooker (PROJECT.md section 4).
 *
 * This is the first part of the C API sketched in PROJECT.md section 5, and
 * it follows those rules: opaque handle in a create/free pair, results by
 * return code, outputs through pointers.
 *
 * File format, version 5. All values are little-endian; a sentinel double in
 * the header catches a machine where that is not true, along with any other
 * disagreement about how a double is laid out.
 *
 *   offset  size            content
 *   0       8               magic "SSEPH\0\0\0"
 *   8       4               uint32 version
 *   12      4               uint32 body count
 *   16      4               uint32 interval count
 *   20      4               uint32 coefficients per position component
 *   24      4               uint32 coefficients per orientation component,
 *                           0 if no body in this asset carries orientation
 *   28      8               double first epoch, seconds from J2000 TDB
 *   36      8               double interval length, seconds
 *   44      8               double sentinel, exactly 1.0
 *   52      variable        per body, in order:
 *                             char   name[32]
 *                             double mu
 *                             double mean radius, metres, 0 if unknown
 *                             double solar flux at 1 AU, W/m^2, 0 if dark
 *                             uint32 1 if this body carries orientation
 *                             uint32 harmonic degree, 0 for a point mass
 *                             if degree >= 2:
 *                               double reference radius, metres
 *                               double C[(d+1)(d+2)/2], triangular
 *                               double S[(d+1)(d+2)/2], same order
 *                             uint32 atmosphere layer count, 0 for airless
 *                             per layer, ascending by altitude:
 *                               double base altitude above the mean radius, m
 *                               double density at that altitude, kg/m^3
 *                               double scale height, metres
 *   ...     8 each          position coefficients, ordered
 *                           [interval][body][component x,y,z][coefficient]
 *   ...     8 each          orientation coefficients, ordered
 *                           [interval][body carrying one, in body order]
 *                           [component w,x,y,z][coefficient]
 *
 * Positions only. Velocity is the analytic derivative of the same polynomial,
 * as in SPICE type 2 - see cheb_eval_deriv.
 *
 * Version 2 added the harmonic block (ROADMAP K4b). It is what makes a body's
 * shape a property of the asset rather than of whoever propagates through it,
 * which PROJECT.md section 4 asks for directly ("ступінь розкладу — параметр
 * ассета тіла"), and it has a consequence worth stating: the coefficients
 * written here are the ones the cooker itself integrated the bodies under, so
 * a vessel and the bodies cannot disagree about the shape of the Earth. A
 * caller cannot get that wrong because a caller is no longer asked.
 *
 * Version 3 added radius and flux (ROADMAP K6b), for the same reason and by
 * the same argument. A conical shadow needs to know how big the Sun is and
 * how big the thing in front of it is, and radiation pressure needs to know
 * how brightly the Sun burns; all three are properties of the system, so they
 * belong to the file that describes the system.
 *
 * The alternative was a header field naming which body is the Sun. Making the
 * flux per body instead removes the question: a vessel feels radiation
 * pressure from every body whose flux is positive, which is one body in every
 * asset we will ever cook, and the code has no notion of "the Sun" to get
 * wrong. Radius is likewise per body and optional - zero means the asset does
 * not say, so the body casts no shadow rather than casting one of an invented
 * size.
 *
 * Neither field touches the Chebyshev coefficients, and that is checkable
 * rather than merely expected: recooking the fixture across this version bump
 * left every determinism hash where it was and only changed the file's
 * length.
 *
 * Version 4 added orientation (ROADMAP K3b): a unit quaternion per body per
 * interval, fitted the same way the position is, carrying the body-fixed
 * frame the tesseral terms of K5 and the co-rotating atmosphere of K7 will
 * need. Quaternion rather than matrix because a matrix's nine components,
 * fitted independently, drift out of orthogonality between the nodes, while
 * a quaternion's four leave exactly one invariant to restore - and restoring
 * it is a sqrt, which is inside what the runtime is allowed to compute.
 *
 * It has one property the position channels do not, and it is not a detail:
 * orientation needs FAR more coefficients per interval than position does,
 * because it oscillates rather than curving. Earth's quaternion turns
 * through half a turn a day, so over the fixture's 8-day interval it is four
 * full cycles of a wave, and a polynomial cannot follow a wave it cannot
 * resolve. Measured, the failure is a cliff and not a slope: degree 24 is
 * wrong by 1.4 radians, degree 26 by 8.8 m at the equator, degree 32 by a
 * millimetre and degree 36 by a micrometre. The fixture uses 36; the cooker
 * measures the error at the least constrained point of every interval and
 * reports it (EphBuildReport::max_orient_error_rad) precisely because a
 * degree chosen just past that cliff would look fine until someone
 * lengthened the interval.
 *
 * Bodies with no orientation model carry no channels at all rather than a
 * constant identity, which is eight of the fixture's ten and about four
 * fifths of what the block would otherwise cost. They read back as the
 * identity anyway (eph_body_orientation), so nothing downstream needs to
 * know which bodies were written.
 *
 * Version 5 added the atmosphere (ROADMAP K7b), by the argument K4b made for
 * harmonics and K6b for radius: how thick the air is over a body is a property
 * of that body, not a setting of whoever propagates through it. Putting it in
 * PropConfig instead would let two callers fly past two differently breathable
 * Earths, and neither would be told.
 *
 * Layers rather than a sampled profile, because density falls fourteen orders
 * of magnitude from sea level to 1000 km and no interpolation of the values
 * themselves survives that; core/atmosphere.h has the whole argument. Zero
 * layers means airless, which is nine of the fixture's ten bodies and costs
 * them four bytes each.
 *
 * The altitudes are above the body's MEAN RADIUS - the same radius the shadow
 * geometry uses, and not the reference radius of the harmonics, which for the
 * Earth is a different number (6378137 against 6371010). Two radii in one
 * asset is a thing worth saying out loud rather than discovering. */

#ifndef CORE_EPHEMERIS_H
#define CORE_EPHEMERIS_H

#include "atmosphere.h"
#include "core.h"
#include "harmonics.h"
#include "quat.h"

#include <stddef.h>

#define EPH_MAGIC "SSEPH\0\0\0"
#define EPH_MAGIC_SIZE 8
#define EPH_VERSION 5u
#define EPH_NAME_SIZE 32

typedef struct EphemerisCtx EphemerisCtx;

/* The one allocating pair in the API. PROJECT.md section 5 forbids C from
 * allocating buffers of data; contexts are the stated exception, because the
 * alternative is making the caller guess a size that depends on the file. */
CoreResult eph_load(const char *path, EphemerisCtx **out);
void       eph_free(EphemerisCtx *ctx);

int         eph_body_count(const EphemerisCtx *ctx);
const char *eph_body_name(const EphemerisCtx *ctx, int body);
double      eph_body_mu(const EphemerisCtx *ctx, int body);

/* Mean radius in metres, or 0 where the asset does not say (ROADMAP K6b).
 *
 * Zero is an answer, not a failure, and it has one consequence: a body of
 * unknown size occults nothing (core/srp.h). Inventing a size instead would
 * put a shadow somewhere no data supports, which is the harder error to
 * notice - a vessel would simply be a little cooler than it should be, for
 * years.
 *
 * This is also the number core/prop.h has been waiting for to turn
 * CORE_EVENT_DISTANCE into an altitude event, and the one the atmosphere of
 * K7 will measure its scale height from. */
double      eph_body_radius(const EphemerisCtx *ctx, int body);

/* Solar irradiance at one astronomical unit from this body, W/m^2, or 0 for
 * a body that does not shine (ROADMAP K6b). */
double      eph_body_flux(const EphemerisCtx *ctx, int body);

/* A body's gravity field beyond its point mass, in its own body-fixed frame
 * (ROADMAP K4b). Writes a field with degree 0 - which harmonics_accel treats
 * as no contribution at all - for a body the asset describes as a point mass,
 * so a caller need not ask twice.
 *
 * Returns CORE_ERR_INVALID_ARG only for a bad body index or a NULL out: "this
 * body is round" is an answer, not a failure. */
CoreResult eph_body_harmonics(const EphemerisCtx *ctx, int body,
                              HarmonicsField *out);

/* A body's atmosphere (ROADMAP K7b), with altitudes measured above its mean
 * radius. Writes a model with no layers - which atmosphere_density treats as
 * vacuum - for a body the asset describes as airless, so a caller need not
 * ask twice.
 *
 * Returns CORE_ERR_INVALID_ARG only for a bad body index or a NULL out: "this
 * body has no air" is an answer, not a failure. */
CoreResult eph_body_atmosphere(const EphemerisCtx *ctx, int body,
                               AtmosphereModel *out);

/* Covered time span, seconds from J2000 TDB. */
CoreResult eph_span(const EphemerisCtx *ctx, double *t_begin, double *t_end);

/* Orientation of a body at t: the quaternion rotating a vector's components
 * from that body's body-fixed frame to the ephemeris frame (quat.h's
 * convention), read back from the fitted channels and renormalised, since a
 * polynomial fit is only approximately unit length between its nodes.
 *
 * A body the asset carries no orientation for returns the identity and
 * CORE_OK - "this body's rotation is not modelled" is an answer, the same
 * way degree 0 is an answer from eph_body_harmonics. Only a bad index, a
 * NULL out, or a time outside the span are errors.
 *
 * The rotation this returns is the one the cooker itself integrated the
 * bodies under, for the same reason the harmonics are (ROADMAP K4b): a
 * vessel and the bodies cannot end up disagreeing about which way a planet
 * is facing. */
CoreResult eph_body_orientation(const EphemerisCtx *ctx, int body, double t,
                                Quat *out);

/* Angular velocity of a body at t, rad/s, in the ephemeris frame (ROADMAP
 * K7b) - the vector a co-rotating atmosphere's wind is built from.
 *
 * Taken from the analytic derivative of the same fitted quaternion the
 * orientation comes from, the way a body's velocity comes from the derivative
 * of its position fit. A body with no orientation model does not turn, and
 * returns the zero vector with CORE_OK.
 *
 * This is what the co-rotating atmosphere of K7 needed the K3b block for, and
 * it is the first thing in the core to read those channels in anger. */
CoreResult eph_body_angular_velocity(const EphemerisCtx *ctx, int body,
                                     double t, Vec3d *out);

/* Position and velocity of a body. Returns CORE_ERR_INVALID_ARG for an
 * unknown body or a time outside the span: extrapolating a Chebyshev fit
 * produces confident nonsense, and a caller that has run off the end of the
 * ephemeris has a problem worth hearing about. */
CoreResult eph_body_state(const EphemerisCtx *ctx, int body, double t,
                          State *out);

#endif /* CORE_EPHEMERIS_H */
