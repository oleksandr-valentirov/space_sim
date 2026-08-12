/* Richardson's third-order analytic halo approximation (ROADMAP C2b).
 *
 * Differential correction needs somewhere to start. Taking that start from a
 * published catalogue works, and C2b does exactly that first, but it only
 * works for systems somebody has already catalogued. This is the version that
 * needs nothing but the mass ratio: give it a libration point and how far out
 * of the plane you want to be, and it produces a state and a period that are
 * wrong by a percent or so and right enough for Newton to finish.
 *
 * The series is Richardson (1980), third order in the amplitudes, in
 * libration-point-centred coordinates scaled by the distance from the point to
 * its nearer primary. It is a small-amplitude expansion and it says so
 * honestly: past a certain amplitude the constraint that fixes the in-plane
 * amplitude from the out-of-plane one has no solution at all, and this returns
 * an error rather than a number. Measured for Earth-Moon L2: solutions exist
 * up to |z| of about 0.067, roughly a third of the largest halo in the JPL
 * catalogue. Larger members are reached by continuation from a smaller one
 * (halo_family), which is how it is done in practice anyway.
 *
 * ---
 *
 * Note what is NOT here: sin and cos. The series is trigonometric in the phase
 * along the orbit, but the only phase this function ever evaluates is the
 * perpendicular crossing of the xz-plane, where every trigonometric term is
 * exactly -1, 0 or +1. What is left is +, -, *, / and sqrt, so this belongs in
 * the deterministic runtime rather than in the cooker (CLAUDE.md invariant 3),
 * and the game can generate halo seeds for any system it invents, in-process
 * and bit-identically across platforms. Evaluating the series anywhere else
 * along the orbit would need libm and would have to move; there is no reason
 * to want that, since what a corrector needs is the crossing. */

#ifndef CORE_RICHARDSON_H
#define CORE_RICHARDSON_H

#include "core.h"

/* Approximate a halo orbit about a collinear libration point.
 *
 * point is 1 or 2. L3 is not implemented: its halo family is not used for
 * anything, and shipping a third coefficient set that nothing in the tests
 * exercises would be worse than not shipping it.
 *
 * az is the out-of-plane displacement at the crossing, in the dimensionless
 * units of the CR3BP - the same units as everything else here, so it is
 * directly comparable with the z of a catalogue orbit. Its SIGN chooses the
 * branch: positive for northern, negative for southern. Its magnitude is
 * approached rather than met, since the third-order terms shift it by a few
 * percent; the returned state's z is what was actually achieved, and that is
 * the value to hand to halo_correct with HALO_HOLD_Z.
 *
 * What counts as a reasonable az scales with gamma, and gamma varies by two
 * orders of magnitude between systems: it is 0.168 for Earth-Moon L2 and
 * 0.010 for Sun-Earth L2. Amplitudes that cover most of the Earth-Moon family
 * are therefore entirely outside the Sun-Earth one, and asking for them
 * returns an error rather than nonsense. As a rule of thumb the series reaches
 * to roughly 0.4 gamma. The units stay absolute anyway, because that is what
 * makes the result comparable with a catalogue and directly usable as the
 * held variable in halo_correct.
 *
 * The returned state sits on the xz-plane with the velocity perpendicular to
 * it - exactly the form halo_correct expects - at t = 0. period is the
 * approximation's own estimate, which is its weakest output: measured 15% low
 * for Earth-Moon L2, because the halo family does not shrink to a point as the
 * out-of-plane amplitude goes to zero and the in-plane amplitude stays large
 * enough to hurt a third-order series. It is still good enough for what it is
 * used for, which is bracketing the crossing.
 *
 * Returns CORE_ERR_TOLERANCE_NOT_MET when the requested amplitude is beyond
 * the series' reach. */
CoreResult richardson_halo(double mu, int point, double az,
                           State *out, double *period);

#endif /* CORE_RICHARDSON_H */
