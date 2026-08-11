/* Shared definitions for the numerical core.
 *
 * Boundary contract with Rust: see PROJECT.md section 5. Errors are returned
 * as result codes, never through global state. */

#ifndef CORE_H
#define CORE_H

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
