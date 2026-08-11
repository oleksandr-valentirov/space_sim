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
