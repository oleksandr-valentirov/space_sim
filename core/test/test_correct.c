/* Differential correction of halo orbits (ROADMAP C2b).
 *
 * C2a integrated somebody else's orbit. Here the orbit is found: a crude guess
 * goes in, a periodic orbit comes out, and it is compared against the JPL
 * catalogue afterwards rather than seeded from it.
 *
 * The strongest check in this file is the family one, and it is worth saying
 * why. Matching a single published orbit could be luck in the seed. Walking
 * the family and finding that the resulting curve brackets the catalogue's
 * members in x, vy, period and Jacobi constant simultaneously is a statement
 * about the whole one-parameter family, and it needs no tolerance at all -
 * only that the published point falls between two computed neighbours.
 *
 * Run from the repository root. */

#include "correct.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16
#define MAX_FAMILY 40

static RefHalo orbit[MAX_ORBITS];
static size_t n_orbits;
static double mu;
static HaloOrbit family[MAX_FAMILY];

static HaloCorrectConfig config(HaloHold hold)
{
    HaloCorrectConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.hold = hold;
    cfg.tol = 1e-11;
    cfg.integrator_tol = 1e-13;
    cfg.max_iterations = 30;
    cfg.max_step = 0.05;
    return cfg;
}

static int between(double a, double b, double x)
{
    return (x >= a && x <= b) || (x >= b && x <= a);
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK ||
        refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    HaloCorrectConfig cfg = config(HALO_HOLD_Z);

    /* A published initial condition is already a fixed point of the
     * correction, so correcting it must move nothing. This separates two
     * failures that would otherwise look alike: a corrector that converges to
     * the wrong orbit fails here, a corrector that cannot converge at all
     * fails in the next block. Orbit 4 is excluded throughout - it is the
     * near-rectilinear member that passes 1708 km below the lunar surface, and
     * core/test/test_halo.c documents why it behaves differently. */
    for (size_t i = 0; i < 4; i++) {
        HaloOrbit found;
        CHECK(halo_correct(mu, &orbit[i].s, orbit[i].period, &cfg, &found)
              == CORE_OK);
        CHECK(found.iterations == 1);
        CHECK(found.residual < 1e-11);
        CHECK(fabs(found.s.r.x - orbit[i].s.r.x) < 1e-15);
        CHECK(fabs(found.period - orbit[i].period) < 1e-11);
    }

    /* Displace the seed and watch it come back. Measured: four to six
     * iterations to a residual near 1e-14, recovering the published state to
     * about 2e-11 - which is the accuracy the catalogue itself is quoted to,
     * not a limit of the corrector. */
    for (size_t i = 0; i < 4; i++) {
        double nudges[2] = { 1e-4, 2e-3 };

        for (int k = 0; k < 2; k++) {
            State seed = orbit[i].s;
            seed.r.x += nudges[k];

            HaloOrbit found;
            CHECK(halo_correct(mu, &seed, orbit[i].period, &cfg, &found)
                  == CORE_OK);
            CHECK(found.iterations > 1);
            CHECK(found.iterations <= 10);
            CHECK(found.residual < 1e-10);   /* the ROADMAP C2b criterion */
            CHECK(fabs(found.s.r.x - orbit[i].s.r.x) < 1e-9);
            CHECK(fabs(found.s.v.y - orbit[i].s.v.y) < 1e-9);
            CHECK(fabs(found.period - orbit[i].period) < 1e-8);
        }
    }

    /* Found rather than reproduced: the only thing taken from the catalogue is
     * z, which is the family parameter and therefore says which orbit is
     * wanted, not what it is. x and vy start at round numbers a long way off -
     * 1.10 against 1.169 for orbit 1151, and -0.18 against -0.194.
     *
     * Measured: orbit 767 recovered in 6 iterations to 2.4e-14 in x, orbit
     * 1151 in 15 iterations to 6.4e-14. */
    for (size_t i = 2; i < 4; i++) {
        State seed;
        memset(&seed, 0, sizeof seed);
        seed.r.x = 1.10;
        seed.r.z = orbit[i].s.r.z;
        seed.v.y = -0.18;

        HaloOrbit found;
        CHECK(halo_correct(mu, &seed, 2.5, &cfg, &found) == CORE_OK);
        CHECK(found.residual < 1e-10);
        CHECK(fabs(found.s.r.x - orbit[i].s.r.x) < 1e-10);
        CHECK(fabs(found.s.v.y - orbit[i].s.v.y) < 1e-10);
        CHECK(fabs(found.period - orbit[i].period) < 1e-10);
        CHECK(fabs(found.jacobi - orbit[i].jacobi) < 1e-10);
    }

    /* The same crude seed at orbit 383's z converges to a different orbit than
     * orbit 383 - x is 1.109 against the catalogue's 1.046 and the period is
     * 2.756 against 1.840. That is the family being folded in z, not a
     * failure: past the fold a single z has two members, and Newton finds
     * whichever one its seed is nearest. Recorded here because it is the
     * behaviour most likely to be mistaken for a bug later. */
    {
        State seed;
        memset(&seed, 0, sizeof seed);
        seed.r.x = 1.10;
        seed.r.z = orbit[1].s.r.z;
        seed.v.y = -0.18;

        HaloOrbit found;
        CHECK(halo_correct(mu, &seed, 2.5, &cfg, &found) == CORE_OK);
        CHECK(found.residual < 1e-10);
        CHECK(fabs(found.s.r.x - orbit[1].s.r.x) > 0.05);
        CHECK(fabs(found.period - orbit[1].period) > 0.5);
    }

    /* The symmetry is imposed, not assumed: a seed off the plane produces a
     * state exactly on it. */
    {
        State seed = orbit[0].s;
        seed.r.y = 0.01;
        seed.v.x = 0.02;
        seed.v.z = -0.03;
        seed.t = 5.0;

        HaloOrbit found;
        CHECK(halo_correct(mu, &seed, orbit[0].period, &cfg, &found)
              == CORE_OK);
        CHECK_BITS_EQ(found.s.r.y, 0.0);
        CHECK_BITS_EQ(found.s.v.x, 0.0);
        CHECK_BITS_EQ(found.s.v.z, 0.0);
        CHECK_BITS_EQ(found.s.t, 0.0);
    }

    /* Continuation, and the check the whole file is built around. */
    {
        HaloOrbit start;
        CHECK(halo_correct(mu, &orbit[0].s, orbit[0].period, &cfg, &start)
              == CORE_OK);

        size_t count = 0;
        CHECK(halo_family(mu, &start, 0.004, &cfg, family, MAX_FAMILY, &count)
              == CORE_OK);
        CHECK(count == MAX_FAMILY);

        for (size_t i = 0; i < count; i++) {
            CHECK(family[i].residual < 1e-10);
            CHECK(family[i].period > 0.0);
        }

        /* The Jacobi constant rises monotonically along this branch, which is
         * what a family looks like and what a scatter of independently
         * converged orbits would not. */
        for (size_t i = 1; i < count; i++) {
            CHECK(family[i].jacobi > family[i - 1].jacobi);
            CHECK(family[i].s.r.z > family[i - 1].s.r.z);
        }

        /* And the curve passes through the published orbits. No tolerance
         * here: the catalogue value simply has to fall between two computed
         * neighbours, in every quantity at once. */
        for (size_t k = 2; k < 4; k++) {
            int found_bracket = 0;

            for (size_t i = 0; i + 1 < count; i++) {
                if (!between(family[i].s.r.z, family[i + 1].s.r.z,
                             orbit[k].s.r.z)) {
                    continue;
                }
                found_bracket = 1;
                CHECK(between(family[i].s.r.x, family[i + 1].s.r.x,
                              orbit[k].s.r.x));
                CHECK(between(family[i].s.v.y, family[i + 1].s.v.y,
                              orbit[k].s.v.y));
                CHECK(between(family[i].period, family[i + 1].period,
                              orbit[k].period));
                CHECK(between(family[i].jacobi, family[i + 1].jacobi,
                              orbit[k].jacobi));
                break;
            }

            CHECK(found_bracket);
        }

        /* Walking the other way stops immediately, and this is the reason
         * HALO_HOLD_X exists. Orbit 0 is near the extreme of |z| on this
         * branch, so a step outward asks for an orbit that is not there and
         * no amount of Newton will find one. Holding x instead crosses the
         * fold without noticing it: 20 members in both directions.
         *
         * A corrector that reported success here would be the more dangerous
         * outcome, so the failure is checked as a feature. */
        size_t backwards = 0;
        CHECK(halo_family(mu, &start, -0.004, &cfg, family, MAX_FAMILY,
                          &backwards) == CORE_OK);
        CHECK(backwards == 0);

        HaloCorrectConfig hold_x = config(HALO_HOLD_X);
        for (int dir = 0; dir < 2; dir++) {
            size_t n = 0;
            CHECK(halo_family(mu, &start, dir ? 0.004 : -0.004, &hold_x,
                              family, 20, &n) == CORE_OK);
            CHECK(n == 20);
            for (size_t i = 0; i < n; i++) {
                CHECK(family[i].residual < 1e-10);
            }
        }
    }

    /* Argument checking, and the failure path. */
    {
        HaloOrbit found;
        size_t count = 0;

        CHECK(halo_correct(mu, NULL, 1.0, &cfg, &found)
              == CORE_ERR_INVALID_ARG);
        CHECK(halo_correct(0.0, &orbit[0].s, 1.0, &cfg, &found)
              == CORE_ERR_INVALID_ARG);
        CHECK(halo_correct(mu, &orbit[0].s, 0.0, &cfg, &found)
              == CORE_ERR_INVALID_ARG);
        CHECK(halo_family(mu, &family[0], 0.0, &cfg, family, MAX_FAMILY,
                          &count) == CORE_ERR_INVALID_ARG);

        HaloCorrectConfig bad = cfg;
        bad.hold = (HaloHold)7;
        CHECK(halo_correct(mu, &orbit[0].s, 1.0, &bad, &found)
              == CORE_ERR_INVALID_ARG);

        bad = cfg;
        bad.tol = 0.0;
        CHECK(halo_correct(mu, &orbit[0].s, 1.0, &bad, &found)
              == CORE_ERR_INVALID_ARG);

        /* A seed nowhere near the family, with the iteration count cut short
         * so the test stays quick. */
        State nonsense;
        memset(&nonsense, 0, sizeof nonsense);
        nonsense.r.x = 0.4;
        nonsense.r.z = 0.3;
        nonsense.v.y = 2.0;

        HaloCorrectConfig short_run = cfg;
        short_run.max_iterations = 3;
        CHECK(halo_correct(mu, &nonsense, 2.0, &short_run, &found)
              == CORE_ERR_TOLERANCE_NOT_MET);
    }

    return TEST_RESULT();
}
