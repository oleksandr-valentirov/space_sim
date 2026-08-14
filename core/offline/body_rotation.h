/* Body orientation from IAU pole and prime-meridian elements - OFFLINE ONLY
 * (ROADMAP K3).
 *
 * Building a rotation from mean pole right ascension/declination and a
 * prime-meridian angle needs sin/cos, so - same rule as cheb_fit.h - this
 * lives on the libm side of the boundary and hands the runtime only the
 * quaternion that comes out, for it to fit and read back with no
 * trigonometry of its own.
 *
 * Source: NAIF generic PCK pck00011.tpc (created 2022-12-27),
 * https://naif.jpl.nasa.gov/pub/naif/generic_kernels/pck/pck00011.tpc -
 * BODY399_POLE_RA/POLE_DEC/PM (Earth) and BODY301_POLE_RA/POLE_DEC/PM
 * (Moon, per that file still sourced from the 2009 IAU report - the 2015
 * WGCCRE report deliberately removed both Earth's approximate expressions
 * and the Moon's low-precision series "to avoid confusion" with the
 * IERS-grade models precise work should use instead).
 *
 * Two things this deliberately does not model, both named rather than
 * silently absent - see body_rotation.c:
 *
 *   - Earth: the file itself documents an error of at least 150 arcseconds
 *     in the prime meridian from this expression alone.
 *   - Moon: no physical libration (the periodic wobble the full IAU model
 *     corrects for, arcminute scale) - only the mean pole and mean
 *     rotation.
 *
 * Neither is invented to fill a gap; both are the cited mean model with a
 * named, bounded piece left out, which is a different thing from a guess. */

#ifndef CORE_BODY_ROTATION_H
#define CORE_BODY_ROTATION_H

#include "core.h"
#include "quat.h"

/* Orientation of `name` at t (seconds from J2000 TDB, the same clock the
 * ephemeris integrates on): the quaternion that rotates a vector's
 * components from that body's body-fixed frame to the inertial frame the
 * ephemeris uses (quat.h's convention).
 *
 * A body with no cited model - everything but Earth and Moon today -
 * returns the identity quaternion rather than failing, which is how "not
 * modelled" reads back once this is baked into the asset (ROADMAP K3): a
 * caller checking whether a body's orientation means anything has nothing
 * to check against but the name, same as it would checking mu > 0.0
 * elsewhere in this codebase for "no GM configured". */
CoreResult body_rotation_of(const char *name, double t, Quat *out);

#endif /* CORE_BODY_ROTATION_H */
