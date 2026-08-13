/* CR3BP: equilibria, the Jacobi constant, and what they prove (ROADMAP C1).
 *
 * Everything here is dimensionless, including the integrator tolerance. The
 * Dop853Config field is named tol_m because in every other use it is metres;
 * here it is units of the primaries' separation. Worth knowing before reading
 * a tolerance of 1e-12 as absurd. */

#include "cr3bp.h"
#include "integrator.h"
#include "test.h"

#include <math.h>
#include <string.h>

/* GM values from data/horizons/gm.csv. */
#define GM_EARTH 398600.435436
#define GM_MOON  4902.800066

#define TWO_PI 6.28318530717958647692

static double jacobi_drift(const State *start, double mu, double revolutions,
                           double tol, long *steps_out)
{
    Cr3bpCtx ctx = { mu };

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol;
    cfg.max_steps = 50000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    State end;
    CoreResult r = dop853_integrate(accel_cr3bp, &ctx, start,
                                    revolutions * TWO_PI, &cfg, &st, &end);
    CHECK(r == CORE_OK);
    if (r != CORE_OK) {
        return 1.0;
    }

    if (steps_out != NULL) {
        *steps_out = st.n_accepted;
    }

    double c0 = cr3bp_jacobi(start->r, start->v, mu);
    double c1 = cr3bp_jacobi(end.r, end.v, mu);
    return fabs((c1 - c0) / c0);
}

int main(void)
{
    double mu = cr3bp_mu(GM_EARTH, GM_MOON);
    Cr3bpCtx ctx = { mu };

    /* Measured 0.012150584269542 for the Earth-Moon system. */
    CHECK(fabs(mu - 0.012150584269542) < 1e-15);
    CHECK_BITS_EQ(cr3bp_mu(1.0, 1.0), 0.5);
    CHECK_BITS_EQ(cr3bp_mu(0.0, 0.0), 0.0);

    Vec3d lagrange[6];
    for (int p = 1; p <= 5; p++) {
        CHECK(cr3bp_lagrange(mu, p, &lagrange[p]) == CORE_OK);
    }

    /* The strongest statement available about the Lagrange points, and it
     * relies on no remembered constants: a particle placed at rest at one
     * must feel no acceleration. Measured residuals run 3.1e-16 to 1.5e-15,
     * which is machine precision on quantities of order one.
     *
     * A test against published x-values would only confirm that the
     * literature and I agree about what those values are. This confirms the
     * equations of motion and the root finder agree with each other. */
    for (int p = 1; p <= 5; p++) {
        Vec3d a;
        accel_cr3bp(0.0, lagrange[p], vec3_zero(), &ctx, &a);
        CHECK(vec3_norm(a) < 1e-14);
    }

    /* Values, for the record and as a guard against a silent change of
     * convention - which primary sits where, and which side is positive.
     * Measured: 0.836915132366261, 1.155682160290809, -1.005062645251943. */
    {
        CHECK(fabs(lagrange[1].x - 0.836915132366261) < 1e-12);
        CHECK(fabs(lagrange[2].x - 1.155682160290809) < 1e-12);
        CHECK(fabs(lagrange[3].x - (-1.005062645251943)) < 1e-12);

        for (int p = 1; p <= 3; p++) {
            CHECK_BITS_EQ(lagrange[p].y, 0.0);
            CHECK_BITS_EQ(lagrange[p].z, 0.0);
        }

        /* L1 sits between the primaries, L2 beyond the secondary, L3 beyond
         * the primary on the far side. */
        CHECK(lagrange[1].x > -mu && lagrange[1].x < 1.0 - mu);
        CHECK(lagrange[2].x > 1.0 - mu);
        CHECK(lagrange[3].x < -mu);
    }

    /* L4 and L5 are equilateral by construction, so both distances are
     * exactly one. This is the check that the primaries are where the header
     * says they are: at -mu and 1-mu, not at 0 and 1. */
    {
        for (int p = 4; p <= 5; p++) {
            double d1 = vec3_norm(vec3_sub(lagrange[p], vec3(-mu, 0.0, 0.0)));
            double d2 = vec3_norm(vec3_sub(lagrange[p], vec3(1.0 - mu, 0.0, 0.0)));
            CHECK(fabs(d1 - 1.0) < 1e-15);
            CHECK(fabs(d2 - 1.0) < 1e-15);
        }
        CHECK_BITS_EQ(lagrange[4].x, lagrange[5].x);
        CHECK_BITS_EQ(lagrange[4].y, -lagrange[5].y);
    }

    /* Jacobi values order C1 > C2 > C3 > C4 = C5. That ordering is the whole
     * reason the L1 gateway opens first as energy rises, which is what makes
     * transfers through it cheap - it is gameplay, not trivia.
     * Measured 3.188341105, 3.172160450, 3.012147149, 2.987997052. */
    {
        double c[6];
        for (int p = 1; p <= 5; p++) {
            c[p] = cr3bp_jacobi(lagrange[p], vec3_zero(), mu);
        }
        CHECK(c[1] > c[2]);
        CHECK(c[2] > c[3]);
        CHECK(c[3] > c[4]);
        CHECK_BITS_EQ(c[4], c[5]);

        CHECK(fabs(c[1] - 3.188341105) < 1e-8);
        CHECK(fabs(c[4] - 2.987997052) < 1e-8);

        /* At the Jacobi value of L1 the forbidden region pinches shut exactly
         * there, so 2*Omega along the axis between the primaries has its
         * minimum at L1 and that minimum equals C(L1). Scanned over a
         * thousand points, the smallest value found must not be below it. */
        double lowest = 1e30;
        for (int i = 1; i < 1000; i++) {
            double x = -mu + (1.0 - mu - (-mu)) * (double)i / 1000.0;
            double two_omega = 2.0 * cr3bp_potential(vec3(x, 0.0, 0.0), mu);
            if (two_omega < lowest) {
                lowest = two_omega;
            }
        }
        CHECK(lowest >= c[1] - 1e-9);
        CHECK(lowest < c[1] + 1e-3);
    }

    /* Zero-velocity curve (ROADMAP G4), built on the same L1 pinch fact
     * rather than a new unverified number: scanning from the barycenter
     * toward the secondary, the boundary must sit before L1 when the gate is
     * shut (c above c[1]), must not exist at all along that ray when the
     * gate is open (c below c[1] - min(2*Omega) there never dips to c, so
     * nothing crosses), and must approach L1 as c approaches c[1] from
     * above, at the sqrt(delta) rate a quadratic minimum implies. */
    {
        double c1 = cr3bp_jacobi(lagrange[1], vec3_zero(), mu);
        Vec3d origin = vec3_zero();
        Vec3d toward_secondary = vec3(1.0, 0.0, 0.0);
        double r_max = 0.95; /* short of the secondary at 1 - mu = 0.9878... */

        double r_shut;
        CoreResult shut = cr3bp_zvc_radius(mu, c1 + 0.01, origin,
                                           toward_secondary, r_max, &r_shut);
        CHECK(shut == CORE_OK);
        CHECK(r_shut > 0.0);
        CHECK(r_shut < lagrange[1].x);

        double r_open;
        CoreResult open = cr3bp_zvc_radius(mu, c1 - 0.01, origin,
                                           toward_secondary, r_max, &r_open);
        CHECK(open == CORE_ERR_TOLERANCE_NOT_MET);

        double r_a, r_b;
        CHECK(cr3bp_zvc_radius(mu, c1 + 0.01, origin, toward_secondary,
                               r_max, &r_a) == CORE_OK);
        CHECK(cr3bp_zvc_radius(mu, c1 + 0.0001, origin, toward_secondary,
                               r_max, &r_b) == CORE_OK);
        double gap_a = lagrange[1].x - r_a;
        double gap_b = lagrange[1].x - r_b;
        CHECK(gap_a > 0.0);
        CHECK(gap_b > 0.0);
        CHECK(gap_b < gap_a);
        /* delta shrank 100x; a quadratic minimum predicts the gap shrinks
         * about sqrt(100) = 10x. Measured: 10.46. Loose bracket, not a
         * precise law - this is a topology sanity check, not a new
         * integrator being validated. */
        double ratio = gap_a / gap_b;
        CHECK(ratio > 4.0);
        CHECK(ratio < 25.0);
    }

    /* The Coriolis term is real, and this is the assertion that would fail if
     * accel_cr3bp quietly ignored its velocity argument - which is the whole
     * reason AccelFunc carries one (PROJECT.md section 4). */
    {
        Vec3d point = vec3(0.6, 0.2, 0.1);
        Vec3d at_rest, moving;
        accel_cr3bp(0.0, point, vec3_zero(), &ctx, &at_rest);
        accel_cr3bp(0.0, point, vec3(0.3, -0.4, 0.5), &ctx, &moving);

        CHECK(!vec3_equal_bits(at_rest, moving));

        /* And it is exactly -2 * omega x v with omega along +z: the x
         * component gains 2*vy and the y component loses 2*vx, with z
         * untouched. */
        CHECK(fabs((moving.x - at_rest.x) - 2.0 * (-0.4)) < 1e-15);
        CHECK(fabs((moving.y - at_rest.y) - (-2.0 * 0.3)) < 1e-15);
        CHECK(fabs(moving.z - at_rest.z) < 1e-15);
    }

    /* Jacobi conservation over 100 revolutions, the Milestone 0 criterion.
     *
     * Measured for a bounded orbit in the L4 region: 1.52e-7, 1.09e-9,
     * 5.60e-11, 1.34e-12, 1.53e-14 at tolerances 1e-6, 1e-8, 1e-10, 1e-12,
     * 1e-14. The drift tracks the tolerance nearly one for one, which is what
     * says the loss is the integrator's and not something structural.
     *
     * This is the sharpest instrument available: the true dynamics hold C
     * exactly, so every digit lost was lost by the numerics. */
    {
        Vec3d l4 = lagrange[4];
        State start = { { l4.x + 0.02, l4.y, 0.0 }, { 0.0, 0.0, 0.0 }, 0.0 };

        long steps_loose = 0, steps_tight = 0;
        double loose = jacobi_drift(&start, mu, 100.0, 1e-8, &steps_loose);
        double tight = jacobi_drift(&start, mu, 100.0, 1e-12, &steps_tight);

        CHECK(tight < 1e-11);
        CHECK(loose > tight * 100.0);
        CHECK(steps_tight > steps_loose);
    }

    /* A harder orbit, passing much closer to the primaries. Measured 2.45e-9
     * at tol 1e-12 - three orders worse than the L4 case for the same
     * tolerance, because close approaches are where a step controller earns
     * its keep. Recorded so the L4 figure is not mistaken for a property of
     * the integrator rather than of the orbit. */
    {
        State start = { { 0.5, 0.0, 0.0 }, { 0.0, 0.6, 0.0 }, 0.0 };
        double drift = jacobi_drift(&start, mu, 100.0, 1e-12, NULL);
        CHECK(drift < 1e-7);
        CHECK(drift > 1e-12);
    }

    /* Argument validation. */
    {
        Vec3d out;
        CHECK(cr3bp_lagrange(mu, 0, &out) == CORE_ERR_INVALID_ARG);
        CHECK(cr3bp_lagrange(mu, 6, &out) == CORE_ERR_INVALID_ARG);
        CHECK(cr3bp_lagrange(mu, 1, NULL) == CORE_ERR_INVALID_ARG);
        CHECK(cr3bp_lagrange(0.0, 1, &out) == CORE_ERR_INVALID_ARG);
        CHECK(cr3bp_lagrange(1.0, 1, &out) == CORE_ERR_INVALID_ARG);

        /* Equal masses put L1 exactly at the midpoint, by symmetry. */
        CHECK(cr3bp_lagrange(0.5, 1, &out) == CORE_OK);
        CHECK(fabs(out.x) < 1e-12);
    }

    return TEST_RESULT();
}
