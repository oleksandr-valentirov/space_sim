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

/* What the asset says about a body beyond where it is (ROADMAP K6b).
 *
 * The name used to be all of it, passed as a bare array of strings. Radius
 * and flux arrived together with the shadow model, and putting them in one
 * struct rather than adding two more parallel arrays is the difference
 * between a caller listing a body and a caller lining up three arrays and
 * hoping they agree.
 *
 * Both new fields are optional and zero is the "not stated" value for each:
 * zero radius means the body occults nothing, zero flux that it does not
 * shine. That is the honest state of most bodies and it is how every test in
 * core/test builds its list. They are written out rather than left to a
 * partial initialiser because the core compiles with
 * -Wmissing-field-initializers as an error - which is the right setting to
 * have when a struct grows a field, and this struct just did.
 *
 * The gravitational parameter is NOT here: it comes from NBodySystem, the
 * system actually being integrated, and the whole argument of K4b was that
 * the asset must record what the cooker used rather than a second copy of
 * it. Radius and flux are different in kind - they change no trajectory in
 * this file, which is why they can be described here and not there. */
typedef struct {
    const char *name;
    double      radius_m;   /* mean radius; 0 if the data does not say */
    double      flux_1au;   /* W/m^2 at 1 AU; 0 for a body that is dark */
} EphBodyInfo;

typedef struct {
    double t_begin;           /* seconds from J2000 TDB */
    double t_end;
    double interval_seconds;  /* one Chebyshev fit per interval */
    size_t degree;            /* coefficients per position component */

    /* Coefficients per quaternion component, or 0 to write no orientation
     * at all (ROADMAP K3b).
     *
     * A separate number from `degree` because it measures something else.
     * Position over an interval is a gentle curve; orientation over the same
     * interval is a wave - Earth turns through four full quaternion cycles
     * in eight days - and a Chebyshev fit follows a wave only once it has
     * enough coefficients to resolve it. Below that it does not degrade, it
     * collapses: measured on the fixture's own 8-day interval, degree 24 is
     * off by 1.4 radians and degree 26 by 4e-7 (8.8 m at the equator).
     * core/ephemeris.h has the whole ladder.
     *
     * Which is also why max_orient_error_rad exists: pick this by measuring,
     * not by argument, and check it again whenever interval_seconds changes,
     * because the cliff moves with it. */
    size_t orient_degree;

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

    /* The same for orientation, in radians, at the least constrained point
     * of each interval: an upper bound on the angle between the rotation the
     * asset gives back and the one body_rotation.c produced. Zero when the
     * asset carries no orientation.
     *
     * Measured as twice the largest quaternion component difference rather
     * than through an angle: an angle near zero comes out of acos with half
     * its digits gone, and this number is meant to be believed down to 1e-13
     * (see EphBuildConfig::orient_degree for what it is guarding against). */
    double max_orient_error_rad;
} EphBuildReport;

/* bodies[] must have sys->n entries and match the order of initial[]. */
CoreResult eph_build(const NBodySystem *sys, const State *initial,
                     const EphBodyInfo *bodies,
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
