/* Refining a Lambert initial guess against the full ephemeris (ROADMAP G3).
 *
 * The flight-planner pipeline PROJECT.md section 8 describes end to end on
 * one real transfer: lambert_solve gives a two-body (Sun-only) initial
 * guess for a departure velocity from Earth's position to Venus's, 70 days
 * later; target_hit then corrects it under the real ten-body field
 * (core/field.h) so the trajectory actually arrives, not just the
 * approximation that ignored every other planet's pull.
 *
 * Uses the committed fixture, not a freshly cooked ephemeris - same reason
 * as core/scenario/sc_trajectory.c: this is a runtime path (reading the
 * asset, the gravity field, the targeting loop), and the fixture is what
 * ships. */

#include "ephemeris.h"
#include "field.h"
#include "lambert.h"
#include "target.h"
#include "test.h"

#include <math.h>
#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define SUN   0
#define VENUS 2
#define EARTH 3

#define DAY 86400.0

int main(void)
{
    EphemerisCtx *eph = NULL;
    CHECK(eph_load(ASSET, &eph) == CORE_OK);
    if (eph == NULL) {
        fprintf(stderr, "test_target: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return TEST_RESULT();
    }

    double t1 = 5.0 * DAY;
    double t2 = 75.0 * DAY;

    State earth_at_t1, venus_at_t2;
    CHECK(eph_body_state(eph, EARTH, t1, &earth_at_t1) == CORE_OK);
    CHECK(eph_body_state(eph, VENUS, t2, &venus_at_t2) == CORE_OK);

    double mu_sun = eph_body_mu(eph, SUN);
    CHECK(mu_sun > 0.0);

    Vec3d r1 = earth_at_t1.r;
    Vec3d r2 = venus_at_t2.r;
    double dt = t2 - t1;

    /* Two-body (Sun-only) initial guess. Whichever branch the real
     * departure direction turns out to be is not the point of this test -
     * core/test/test_lambert.c already covers both - so try short way first
     * and fall back rather than assume. */
    Vec3d v1_guess, v2_guess;
    CoreResult lr = lambert_solve(r1, r2, dt, mu_sun, 1, 0, &v1_guess, &v2_guess);
    if (lr != CORE_OK) {
        lr = lambert_solve(r1, r2, dt, mu_sun, 0, 0, &v1_guess, &v2_guess);
    }
    CHECK(lr == CORE_OK);
    if (lr != CORE_OK) {
        return TEST_RESULT();
    }

    /* Earth and Venus are excluded from the perturbing field, not just the
     * central body left out of it: r1 and r2 sit exactly at those bodies'
     * own positions (they came from eph_body_state above), and point-mass
     * gravity is singular at zero distance. This is also the physically
     * right patched-conic scope - a departing or arriving body's own
     * gravity well is a separate phase this test does not model, same as
     * lambert_solve never sees it either.
     *
     * Built and then narrowed, never assembled by hand. This block used to
     * set four fields directly and leave the rest of FieldCtx as whatever
     * was on the stack - which worked until K7b put an atmosphere and a
     * layer count in there, and then crashed on Windows. field_exclude
     * exists so that the question does not come up again; core/field.h has
     * the whole story. */
    FieldCtx field;
    CHECK(field_all_but(eph, EARTH, &field) == CORE_OK);
    field_exclude(&field, VENUS);
    CHECK(field.n_bodies == eph_body_count(eph) - 2);

    Dop853Config icfg = { 0 };
    icfg.tol_m = 1.0; /* metre - same order as the ephemeris fit itself (data/fixture/README.md) */

    State depart = { r1, v1_guess, t1 };
    TargetReport report;
    CoreResult tr = target_hit(accel_field_var, &field, &icfg, &depart, t2, r2,
                               1.0 /* metre */, 30, &report);

    CHECK(tr == CORE_OK);
    CHECK(field.failed == 0);
    CHECK(report.miss_m < 1.0);

    /* The point of the pipeline: the correction is a refinement, not a
     * re-solve. Measured: guess 37405.0 m/s, correction 246.0 m/s (0.66%
     * relative), converged in 3 iterations - the Sun dominates the field
     * (it is one of the eight bodies left after excluding Earth and Venus),
     * so the other seven planets' pull over 70 days nudges the answer
     * rather than replacing it. */
    double guess_speed = vec3_norm(v1_guess);
    double correction = vec3_distance(depart.v, v1_guess);
    CHECK(guess_speed > 0.0);
    CHECK(correction > 0.0);      /* the two-body guess is not exact */
    CHECK(correction < guess_speed * 0.02); /* but it is close */

    eph_free(eph);
    return TEST_RESULT();
}
