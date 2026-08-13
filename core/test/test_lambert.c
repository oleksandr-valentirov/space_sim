/* Lambert's problem (ROADMAP.md, M3, "Планування"). */

#include "accel.h"
#include "integrator.h"
#include "lambert.h"
#include "test.h"

#include <math.h>

#define MU_EARTH 3.98600435436e14
#define MU_SUN   1.32712440018e20
#define R_LEO    7.0e6
#define AU       1.495978707e11

static int close_rel(double a, double b, double reltol, double abstol)
{
    return fabs(a - b) < reltol * fabs(b) + abstol;
}

static int vec_close_rel(Vec3d a, Vec3d b, double reltol, double abstol)
{
    return close_rel(a.x, b.x, reltol, abstol)
        && close_rel(a.y, b.y, reltol, abstol)
        && close_rel(a.z, b.z, reltol, abstol);
}

/* Propagate s0 forward by dt with the two-body force, using DOP853 - the
 * already-proven runtime integrator (ROADMAP B4) - as ground truth. */
static CoreResult propagate(State s0, double mu, double dt, State *out)
{
    TwoBodyCtx ctx = { mu };
    Dop853Config cfg = { 0 };
    cfg.tol_m = 1e-3; /* millimetre: far tighter than anything checked below */
    Dop853State st = { 0 };
    return dop853_integrate(accel_two_body, &ctx, &s0, s0.t + dt, &cfg, &st, out);
}

/* Fly s0 for a fraction of an orbit, solve Lambert for the resulting chord
 * and time, and check the solved velocities against what was actually
 * propagated.
 *
 * prograde is derived from s0's own angular momentum (h = r0 x v0, conserved
 * along the whole arc) rather than passed in: cross(r1, r2) is always
 * parallel to h - both endpoints lie in the orbital plane, whose normal is
 * h/|h| - and h.z >= 0 is exactly the condition under which lambert_solve's
 * "prograde" convention reproduces the arc that was actually flown, whether
 * it went the short way or the long way around. Working this out and getting
 * it wrong would show up as every "long way" case failing while "short way"
 * passed, which is why both are exercised below. */
static void run_case(const char *label, State s0, double mu, double frac_of_period)
{
    double period = two_body_period(s0.r, s0.v, mu);
    CHECK(period > 0.0);
    double dt = frac_of_period * period;

    State s1;
    CHECK(propagate(s0, mu, dt, &s1) == CORE_OK);

    Vec3d h = two_body_angular_momentum(s0.r, s0.v);
    int prograde = h.z >= 0.0;

    Vec3d v1, v2;
    CoreResult r = lambert_solve(s0.r, s1.r, dt, mu, prograde, 0, &v1, &v2);
    if (r != CORE_OK) {
        fprintf(stderr, "%s: lambert_solve returned %s\n", label,
               core_result_str(r));
        CHECK(r == CORE_OK);
        return;
    }

    CHECK(vec_close_rel(v1, s0.v, 1e-6, 1e-4));
    CHECK(vec_close_rel(v2, s1.v, 1e-6, 1e-4));

    /* Self-consistency, independent of the DOP853 ground truth: both ends of
     * a Lambert solution lie on the same conic, so specific energy and
     * angular momentum computed from (r1, v1) and (r2, v2) separately must
     * agree. */
    double e1 = two_body_energy(s0.r, v1, mu);
    double e2 = two_body_energy(s1.r, v2, mu);
    CHECK(close_rel(e1, e2, 1e-6, 1e-4));

    Vec3d h1 = two_body_angular_momentum(s0.r, v1);
    Vec3d h2 = two_body_angular_momentum(s1.r, v2);
    CHECK(vec_close_rel(h1, h2, 1e-6, 1e-4));
}

static State circular(double radius, double mu)
{
    double v = sqrt(mu / radius);
    State s = { { radius, 0.0, 0.0 }, { 0.0, v, 0.0 }, 0.0 };
    return s;
}

/* Rotate a planar (xy) state about the x-axis by angle rad, tilting its
 * angular momentum away from +z. Used to reach the h.z < 0 branch. */
static State tilted(State s, double rad)
{
    double c = cos(rad), sn = sin(rad);
    State out = s;
    out.r.y = s.r.y * c;
    out.r.z = s.r.y * sn;
    out.v.y = s.v.y * c;
    out.v.z = s.v.y * sn;
    return out;
}

/* Periapsis state of an ellipse with semi-major axis a and eccentricity e,
 * in the xy-plane. */
static State periapsis(double a, double e, double mu)
{
    double rp = a * (1.0 - e);
    double vp = sqrt(mu * (2.0 / rp - 1.0 / a));
    State s = { { rp, 0.0, 0.0 }, { 0.0, vp, 0.0 }, 0.0 };
    return s;
}

int main(void)
{
    /* Short way (swept angle < pi) and long way (> pi) around the same
     * circular LEO orbit - the two branches of the direction convention. */
    run_case("circular short way", circular(R_LEO, MU_EARTH), MU_EARTH, 0.25);
    run_case("circular long way", circular(R_LEO, MU_EARTH), MU_EARTH, 0.70);

    /* Eccentric orbit, starting at periapsis where the two-body dynamics are
     * fastest and DOP853's step control is tested hardest. */
    run_case("eccentric short way",
             periapsis(1.2 * R_LEO, 0.3, MU_EARTH), MU_EARTH, 0.20);
    run_case("eccentric long way",
             periapsis(1.2 * R_LEO, 0.3, MU_EARTH), MU_EARTH, 0.65);

    /* Inclined enough (120 degrees) to flip h.z negative, exercising the
     * prograde = 0 branch instead of assuming the z-up case covers it. */
    {
        double tilt = 2.0943951023931953; /* 120 degrees */
        State s0 = tilted(circular(R_LEO, MU_EARTH), tilt);
        run_case("retrograde-in-z short way", s0, MU_EARTH, 0.3);
        run_case("retrograde-in-z long way", s0, MU_EARTH, 0.6);
    }

    /* Interplanetary scale: same dimensionless z-domain (PROJECT.md section
     * 4 boundary aside), wildly different metres and seconds. Nothing in
     * lambert_solve should care. */
    run_case("heliocentric scale", circular(AU, MU_SUN), MU_SUN, 0.4);

    /* Degenerate inputs, all rejected before any iteration starts. */
    {
        Vec3d r1 = vec3(1.0e7, 0.0, 0.0);
        Vec3d r2 = vec3(2.0e7, 0.0, 0.0); /* collinear with r1 */
        Vec3d v1, v2;
        CHECK(lambert_solve(r1, r2, 3600.0, MU_EARTH, 1, 0, &v1, &v2)
              == CORE_ERR_INVALID_ARG);

        Vec3d r3 = vec3(0.0, 1.0e7, 0.0);
        CHECK(lambert_solve(r1, r3, -1.0, MU_EARTH, 1, 0, &v1, &v2)
              == CORE_ERR_INVALID_ARG);
        CHECK(lambert_solve(r1, r3, 3600.0, MU_EARTH, 1, 1, &v1, &v2)
              == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
