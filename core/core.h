/* Shared definitions for the numerical core.
 *
 * Boundary contract with Rust: see PROJECT.md section 5. Errors are returned
 * as result codes, never through global state. */

#ifndef CORE_H
#define CORE_H

#include "vec3.h"

/* A body or vessel at one instant.
 *
 * Frame: barycentric, inertial, ICRF-like (PROJECT.md section 4). Position in
 * metres, velocity in metres per second, t in seconds from the epoch of the
 * loaded ephemeris. Everything that crosses the FFI boundary uses this
 * layout, so it must stay a plain struct of doubles with no padding
 * surprises. */
typedef struct {
    Vec3d  r;
    Vec3d  v;
    double t;
} State;

/* What a vessel is, beyond where it is (PROJECT.md section 5, ROADMAP K6b).
 *
 * Gravity does not need this - a vessel is a massless test particle in the
 * field of the bodies, and that is the split the architecture rests on
 * (core/field.h). Radiation pressure does: the acceleration it produces
 * scales with Cr*A/m, which is a property of the spacecraft and of nothing
 * else. So a massless test particle feels no sunlight, and K6 is where this
 * struct finally had to exist.
 *
 * core/prop.h used to explain why it did not: a struct whose every field is
 * ignored is worse than its absence, because the caller fills it in, nothing
 * happens, and nothing says so. That argument is why cd, the drag
 * coefficient of the PROJECT.md sketch, is still not here. It arrives with
 * K7, together with the atmosphere that reads it.
 *
 * Zero mass means "no radiation pressure", not an error: it is what a caller
 * that has not thought about SRP yet passes, and it reproduces the point-mass
 * trajectory bit for bit. */
typedef struct {
    double mass_kg;
    double area_m2;   /* cross-section presented to the Sun */
    double cr;        /* 1 absorbs, 2 reflects; real spacecraft near 1.3 */
} VesselParams;

typedef enum {
    CORE_OK = 0,
    CORE_ERR_BUFFER_TOO_SMALL,
    CORE_ERR_TOLERANCE_NOT_MET,
    CORE_ERR_INVALID_ARG,
} CoreResult;

/* Human-readable name of a result code. Returns a static string; never NULL,
 * even for values outside the enum. */
const char *core_result_str(CoreResult r);

#endif /* CORE_H */
