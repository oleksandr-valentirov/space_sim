/* Building the ephemeris asset - OFFLINE ONLY (ROADMAP B6).
 *
 * Integrates the mutual N-body system once, fits each body's position to
 * Chebyshev polynomials over fixed intervals, and writes the file that
 * core/ephemeris.h reads. This runs on the developer's machine and the
 * result ships; it never runs on a player's. That is not an optimisation but
 * the thing that makes determinism possible at all - two machines computing
 * their own coefficients would disagree in the last bits and diverge from
 * there (PROJECT.md section 4). */

#ifndef CORE_EPH_BUILD_H
#define CORE_EPH_BUILD_H

#include "nbody.h"

#include <stddef.h>

typedef struct {
    double t_begin;           /* seconds from J2000 TDB */
    double t_end;
    double interval_seconds;  /* one Chebyshev fit per interval */
    size_t degree;            /* coefficients per component */

    /* Integrator tolerance in metres.
     *
     * Choose it from the fastest body in the set, not from the size of the
     * system. B5 measured why: one global tolerance treats the Moon's 3.8e8 m
     * orbit and Neptune's 4.5e12 m one alike, and the Moon converges two
     * orders of magnitude later than everything else. */
    double tol_m;
} EphBuildConfig;

typedef struct {
    long   integrator_steps;
    size_t intervals;
    size_t bytes_written;

    /* Largest difference between the fitted polynomial and the integrator,
     * checked at points that are not fit nodes. This is the number that says
     * whether the degree and interval length are adequate. */
    double max_fit_error_m;
} EphBuildReport;

/* names[] must have sys->n entries and match the order of initial[]. */
CoreResult eph_build(const NBodySystem *sys, const State *initial,
                     const char *const *names,
                     const EphBuildConfig *cfg,
                     const char *out_path,
                     EphBuildReport *report);

#endif /* CORE_EPH_BUILD_H */
