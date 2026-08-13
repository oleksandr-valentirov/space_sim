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
     * orders of magnitude later than everything else.
     *
     * Choose it so that it binds, which is a separate requirement and the one
     * that is easy to miss. ex_ephspan measured a ten-year cook at falling
     * tolerances and found 1, 1e-1 and 1e-2 m producing the identical step
     * count: below roughly 1e-3 m the steps here are set by the forced
     * landings on fit nodes, not by this field. A cook in that regime is as
     * accurate as its node cadence happens to make it, and changing
     * interval_seconds or degree would silently change its accuracy while
     * max_fit_error_m, which is per-interval, reported nothing.
     *
     * It is not what limits the asset - the Chebyshev fit is, by two orders -
     * because this controls local error that accumulates over the whole span
     * while the fit error does not accumulate at all. Over ten years the
     * accumulated difference between 1e-3 and 1e-6 m is about 18 m of lunar
     * position, against a 7.6e-2 m fit error. Hence a tolerance far below the
     * fit error is not waste. */
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

/* Whether this build of the cooker anchors the barycentre before integrating
 * (nbody_anchor_barycentre). Compiled in, not configurable at run time:
 *
 *     make                            anchored, the default
 *     make ANCHOR_BARYCENTRE=0        not anchored, for comparison
 *
 * A build switch rather than an EphBuildConfig field on purpose. It is not a
 * knob a caller should be choosing per cook - the asset ships one way, and
 * two callers picking differently would be a bug that produced two plausible
 * ephemerides. It exists so the effect can be measured against its own
 * absence, which is the only way the number in nbody.h stays honest.
 *
 * Diagnostics that integrate the same system WITHOUT going through eph_build
 * ask this, so that a comparison against the asset stays a comparison of one
 * thing. That is why it is a function and not a macro the callers test: the
 * setting is compiled into this translation unit alone, and nothing else
 * needs the -D. */
int eph_anchor_enabled(void);

#endif /* CORE_EPH_BUILD_H */
