/* Richardson's third-order halo approximation (ROADMAP C2b).
 *
 * The point of this file is one chain, run at the end: mass ratio in,
 * periodic orbits out, with nothing taken from the JPL catalogue at any step -
 * and the resulting family still passes through the catalogue's published
 * orbits. Everything before it checks a piece of that chain in isolation so a
 * failure says which piece.
 *
 * Run from the repository root. */

#include "correct.h"
#include "refdata.h"
#include "richardson.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16
#define MAX_FAMILY 80

static RefHalo orbit[MAX_ORBITS];
static size_t n_orbits;
static double mu;
static HaloOrbit family[MAX_FAMILY];

static HaloCorrectConfig config(void)
{
    HaloCorrectConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.hold = HALO_HOLD_Z;
    cfg.tol = 1e-11;
    cfg.integrator_tol = 1e-13;
    cfg.max_iterations = 40;
    cfg.max_step = 0.02;
    return cfg;
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK ||
        refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    HaloCorrectConfig cfg = config();

    /* Shape of the approximation itself, before any integrating. */
    {
        State s;
        double period;
        CHECK(richardson_halo(mu, 2, -0.05, &s, &period) == CORE_OK);

        /* On the plane, perpendicular, as halo_correct requires. */
        CHECK_BITS_EQ(s.r.y, 0.0);
        CHECK_BITS_EQ(s.v.x, 0.0);
        CHECK_BITS_EQ(s.v.z, 0.0);
        CHECK_BITS_EQ(s.t, 0.0);

        /* The sign of az picks the branch, and the magnitude is approached
         * rather than met: measured -0.0577 for a request of -0.05, a 15%
         * overshoot from the third-order terms. */
        CHECK(s.r.z < 0.0);
        CHECK(fabs(s.r.z) > 0.9 * 0.05);
        CHECK(fabs(s.r.z) < 1.3 * 0.05);

        /* Near L2, on the far side from the Moon at this crossing, going the
         * way the catalogue's orbits go. */
        Vec3d l2;
        CHECK(cr3bp_lagrange(mu, 2, &l2) == CORE_OK);
        CHECK(s.r.x > l2.x);
        CHECK(s.r.x - l2.x < 0.1);
        CHECK(s.v.y < 0.0);
        CHECK(period > 2.0);
        CHECK(period < 4.0);
    }

    /* The problem is symmetric in z, so the two branches must be exact
     * mirrors of each other - not merely similar. */
    {
        State north, south;
        double t_north, t_south;
        CHECK(richardson_halo(mu, 2, 0.04, &north, &t_north) == CORE_OK);
        CHECK(richardson_halo(mu, 2, -0.04, &south, &t_south) == CORE_OK);

        CHECK_BITS_EQ(north.r.x, south.r.x);
        CHECK_BITS_EQ(north.r.z, -south.r.z);
        CHECK_BITS_EQ(north.v.y, south.v.y);
        CHECK_BITS_EQ(t_north, t_south);
    }

    /* The amplitude limit is real and is reported rather than guessed at.
     * Measured for Earth-Moon L2: solutions up to |az| near 0.067, nothing
     * beyond. A series that returned a number there would be worse than one
     * that refuses. */
    {
        State s;
        double period;
        CHECK(richardson_halo(mu, 2, -0.06, &s, &period) == CORE_OK);
        CHECK(richardson_halo(mu, 2, -0.07, &s, &period)
              == CORE_ERR_TOLERANCE_NOT_MET);
        CHECK(richardson_halo(mu, 2, -1.0, &s, &period)
              == CORE_ERR_TOLERANCE_NOT_MET);
    }

    /* Seeds converge, at both points and on both branches. The iteration
     * counts are the interesting output: 5 near the top of the amplitude range
     * for L2 and 14 near the bottom, because the in-plane amplitude of a halo
     * does not go to zero with the out-of-plane one, so the small-|z| end is
     * where a third-order series is least at home - the opposite of the
     * intuition that small means easy. */
    for (int point = 1; point <= 2; point++) {
        for (int sign = -1; sign <= 1; sign += 2) {
            for (int step = 1; step <= 6; step++) {
                double az = (double)sign * 0.01 * (double)step;

                State seed;
                double period;
                CHECK(richardson_halo(mu, point, az, &seed, &period)
                      == CORE_OK);

                HaloOrbit found;
                CHECK(halo_correct(mu, &seed, period, &cfg, &found)
                      == CORE_OK);
                CHECK(found.residual < 1e-10);
                CHECK(found.iterations <= 25);

                /* HALO_HOLD_Z holds z, so the orbit found is at exactly the
                 * amplitude the approximation produced. */
                CHECK_BITS_EQ(found.s.r.z, seed.r.z);

                /* The seed was close in position and further off in period -
                 * measured up to 0.05 in x and 20% in period. The corrector
                 * does not use the period as an unknown, which is why the
                 * weaker of the two outputs does no harm. */
                CHECK(fabs(found.s.r.x - seed.r.x) < 0.1);
                CHECK(fabs(found.period - period) < 0.4 * period);
            }
        }
    }

    /* The reach scales with gamma, and gamma varies by two orders of
     * magnitude between systems. The same absolute amplitude that sits
     * comfortably inside the Earth-Moon L2 family is far outside the
     * Sun-Earth one, where gamma is 0.010 rather than 0.168 - so it fails,
     * and a proportionally smaller one succeeds. Worth asserting because
     * getting this wrong looks like a broken series rather than a
     * misunderstood unit. */
    {
        const double mu_sun_earth = 3.003480593992994e-06;

        State s;
        double period;
        CHECK(richardson_halo(mu_sun_earth, 2, -0.05, &s, &period)
              == CORE_ERR_TOLERANCE_NOT_MET);
        CHECK(richardson_halo(mu_sun_earth, 2, -0.002, &s, &period)
              == CORE_OK);

        Vec3d l2;
        CHECK(cr3bp_lagrange(mu_sun_earth, 2, &l2) == CORE_OK);
        CHECK(s.r.x > l2.x);
        CHECK(s.r.z < 0.0);
        CHECK(s.v.y < 0.0);
    }

    /* Argument checking. */
    {
        State s;
        double period;
        CHECK(richardson_halo(mu, 2, 0.0, &s, &period) == CORE_ERR_INVALID_ARG);
        CHECK(richardson_halo(mu, 3, 0.05, &s, &period)
              == CORE_ERR_INVALID_ARG);
        CHECK(richardson_halo(mu, 0, 0.05, &s, &period)
              == CORE_ERR_INVALID_ARG);
        CHECK(richardson_halo(0.0, 2, 0.05, &s, &period)
              == CORE_ERR_INVALID_ARG);
        CHECK(richardson_halo(mu, 2, 0.05, NULL, &period)
              == CORE_ERR_INVALID_ARG);
    }

    /* The chain, and the reason the rest of this file exists.
     *
     * Nothing below reads the catalogue except to compare against it at the
     * end. A third-order series produces a seed, Newton turns the seed into a
     * periodic orbit, continuation walks that orbit out to large amplitudes,
     * and the curve so produced brackets JPL's published orbits in x, vy,
     * period and Jacobi constant at once. */
    {
        State seed;
        double period;
        CHECK(richardson_halo(mu, 2, -0.06, &seed, &period) == CORE_OK);

        HaloOrbit start;
        CHECK(halo_correct(mu, &seed, period, &cfg, &start) == CORE_OK);

        size_t count = 0;
        CHECK(halo_family(mu, &start, -0.004, &cfg, family, MAX_FAMILY, &count)
              == CORE_OK);

        /* Measured 33 members, reaching z = -0.1985 before the branch ends -
         * the same fold documented in core/test/test_correct.c, met from the
         * other side. */
        CHECK(count > 25);
        CHECK(count < MAX_FAMILY);
        CHECK(family[count - 1].s.r.z < -0.19);

        for (size_t i = 0; i < count; i++) {
            CHECK(family[i].residual < 1e-10);
        }

        /* Orbits 767 and 1151 fall inside the range covered. Orbit 0 sits at
         * z = -0.2023, just past where this branch stops, and orbit 383 is on
         * the far side of the fold - neither is a failure, and both are why
         * the loop asserts on a fixed pair rather than on all of them. */
        for (size_t k = 2; k < 4; k++) {
            int bracketed = 0;

            for (size_t i = 0; i + 1 < count; i++) {
                double lo = family[i + 1].s.r.z;
                double hi = family[i].s.r.z;
                if (orbit[k].s.r.z < lo || orbit[k].s.r.z > hi) {
                    continue;
                }
                bracketed = 1;

                CHECK((orbit[k].s.r.x - family[i].s.r.x)
                      * (orbit[k].s.r.x - family[i + 1].s.r.x) <= 0.0);
                CHECK((orbit[k].s.v.y - family[i].s.v.y)
                      * (orbit[k].s.v.y - family[i + 1].s.v.y) <= 0.0);
                CHECK((orbit[k].period - family[i].period)
                      * (orbit[k].period - family[i + 1].period) <= 0.0);
                CHECK((orbit[k].jacobi - family[i].jacobi)
                      * (orbit[k].jacobi - family[i + 1].jacobi) <= 0.0);
                break;
            }

            CHECK(bracketed);
        }
    }

    return TEST_RESULT();
}
