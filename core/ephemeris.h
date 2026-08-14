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
 * File format, version 2. All values are little-endian; a sentinel double in
 * the header catches a machine where that is not true, along with any other
 * disagreement about how a double is laid out.
 *
 *   offset  size            content
 *   0       8               magic "SSEPH\0\0\0"
 *   8       4               uint32 version
 *   12      4               uint32 body count
 *   16      4               uint32 interval count
 *   20      4               uint32 coefficients per component
 *   24      8               double first epoch, seconds from J2000 TDB
 *   32      8               double interval length, seconds
 *   40      8               double sentinel, exactly 1.0
 *   48      variable        per body, in order:
 *                             char   name[32]
 *                             double mu
 *                             uint32 harmonic degree, 0 for a point mass
 *                             if degree >= 2:
 *                               double reference radius, metres
 *                               double C[(d+1)(d+2)/2], triangular
 *                               double S[(d+1)(d+2)/2], same order
 *   ...     8 each          coefficients, ordered
 *                           [interval][body][component x,y,z][coefficient]
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
 * caller cannot get that wrong because a caller is no longer asked. */

#ifndef CORE_EPHEMERIS_H
#define CORE_EPHEMERIS_H

#include "core.h"
#include "harmonics.h"

#include <stddef.h>

#define EPH_MAGIC "SSEPH\0\0\0"
#define EPH_MAGIC_SIZE 8
#define EPH_VERSION 2u
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

/* A body's gravity field beyond its point mass, in its own body-fixed frame
 * (ROADMAP K4b). Writes a field with degree 0 - which harmonics_accel treats
 * as no contribution at all - for a body the asset describes as a point mass,
 * so a caller need not ask twice.
 *
 * Returns CORE_ERR_INVALID_ARG only for a bad body index or a NULL out: "this
 * body is round" is an answer, not a failure. */
CoreResult eph_body_harmonics(const EphemerisCtx *ctx, int body,
                              HarmonicsField *out);

/* Covered time span, seconds from J2000 TDB. */
CoreResult eph_span(const EphemerisCtx *ctx, double *t_begin, double *t_end);

/* Position and velocity of a body. Returns CORE_ERR_INVALID_ARG for an
 * unknown body or a time outside the span: extrapolating a Chebyshev fit
 * produces confident nonsense, and a caller that has run off the end of the
 * ephemeris has a problem worth hearing about. */
CoreResult eph_body_state(const EphemerisCtx *ctx, int body, double t,
                          State *out);

#endif /* CORE_EPHEMERIS_H */
