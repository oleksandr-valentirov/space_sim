/* Body orientation from IAU pole and prime-meridian elements - OFFLINE ONLY
 * (ROADMAP K3).
 *
 * Building a rotation from mean pole right ascension/declination and a
 * prime-meridian angle needs sin/cos, so - same rule as cheb_fit.h - this
 * lives on the libm side of the boundary and hands the runtime only the
 * quaternion that comes out, for it to fit and read back with no
 * trigonometry of its own.
 *
 * Poles: NAIF generic PCK pck00011.tpc (created 2022-12-27),
 * https://naif.jpl.nasa.gov/pub/naif/generic_kernels/pck/pck00011.tpc -
 * BODY399_POLE_RA/POLE_DEC (Earth) and BODY301_POLE_RA/POLE_DEC/PM
 * (Moon, per that file still sourced from the 2009 IAU report - the 2015
 * WGCCRE report deliberately removed both Earth's approximate expressions
 * and the Moon's low-precision series "to avoid confusion" with the
 * IERS-grade models precise work should use instead).
 *
 * Earth's prime meridian is NOT from that file (ROADMAP K3b). It is the
 * IAU 2000 Earth Rotation Angle, and body_rotation.c derives the two
 * constants of the 3-1-3 sequence from it rather than restating them. What
 * pck00011.tpc gives instead is an approximate expression its own header
 * warns is off by "at least 150 arcseconds"; measured, it is 169 arcsec at
 * J2000 and 1842 by J2200, because its rate is wrong as well as its phase.
 * Both numbers, and why K3a reported the first as 1129, are in
 * body_rotation.c.
 *
 * Three things this deliberately does not model, all named rather than
 * silently absent - see body_rotation.c:
 *
 *   - Earth: UT1 is taken to run at a fixed offset from the ephemeris
 *     clock, the value it had at J2000. The real offset drifts, by roughly
 *     0.7 s a year at the rate obj_earth.txt's "Mean solar day 2000.0, s =
 *     86400.002" implies, and it drifts unpredictably - no model can know
 *     where a future leap second falls. So this is the part of Earth's
 *     rotation nobody can supply, not a corner cut: about 11 arcsec of
 *     meridian per year of extrapolation from the epoch.
 *   - Earth: no polar motion and no nutation (both sub-arcsecond, and both
 *     needing tables this asset does not carry).
 *   - Moon: no physical libration (the periodic wobble the full IAU model
 *     corrects for, arcminute scale) - only the mean pole and mean
 *     rotation.
 *
 * None is invented to fill a gap; each is the cited mean model with a
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

/* Whether this body has a cited model at all, i.e. whether
 * body_rotation_of returns anything but the identity.
 *
 * The cooker asks so it can leave the orientation channels out of the asset
 * for a body it would only ever fill with a constant identity (ROADMAP
 * K3b): eight of the fixture's ten bodies, and about four fifths of what
 * those channels would otherwise cost. A body without a model reads back as
 * the identity from the asset too, so nothing downstream has to know which
 * bodies were written. */
int body_rotation_has_model(const char *name);

#endif /* CORE_BODY_ROTATION_H */
