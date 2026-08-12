/* Reproducing published halo orbits (ROADMAP C2a).
 *
 * C2 is split in two on purpose. This half integrates somebody else's orbit
 * and checks that it comes back to where it started - a pure test of the
 * integrator, with no search machinery of our own involved. If a published
 * orbit does not close, the fault is in C1 and that is where to look. Only
 * once this passes is it worth writing differential correction.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "integrator.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16

/* One unit of length in the Earth-Moon system, km, from the same JPL API that
 * supplied the orbits. */
#define LUNIT_KM 389703.264829278
#define MOON_RADIUS_KM 1737.1

static RefHalo orbit[MAX_ORBITS];
static size_t n_orbits;
static double mu;

/* Position and velocity error after integrating exactly one period. */
static double closure(const RefHalo *h, double tol)
{
    Cr3bpCtx ctx = { mu };

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol;
    cfg.max_steps = 20000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    State end;
    CoreResult r = dop853_integrate(accel_cr3bp, &ctx, &h->s, h->period,
                                    &cfg, &st, &end);
    CHECK(r == CORE_OK);
    if (r != CORE_OK) {
        return 1.0;
    }

    return vec3_distance(end.r, h->s.r)
         + vec3_norm(vec3_sub(end.v, h->s.v));
}

/* Closest approach to the secondary over one period, in units of the
 * primaries' separation. */
static double perilune(const RefHalo *h)
{
    Cr3bpCtx ctx = { mu };
    Vec3d moon = vec3(1.0 - mu, 0.0, 0.0);

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-11;
    cfg.max_steps = 20000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    State current = h->s;
    double closest = vec3_distance(current.r, moon);

    for (int k = 1; k <= 1000; k++) {
        State next;
        double t = h->period * (double)k / 1000.0;
        if (dop853_integrate(accel_cr3bp, &ctx, &current, t, &cfg, &st, &next)
            != CORE_OK) {
            return -1.0;
        }
        current = next;

        double d = vec3_distance(current.r, moon);
        if (d < closest) {
            closest = d;
        }
    }

    return closest;
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK ||
        refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    CHECK(n_orbits == 5);
    CHECK(orbit[0].index == 0);
    CHECK(orbit[4].index == 1534);

    /* The mass ratio travels with the orbits rather than being recomputed,
     * and this is why: JPL's catalogue uses 1.215058560962404e-02 while the
     * value derived from data/horizons/gm.csv is 0.012150584269542. They part
     * company in the eighth digit, and a published initial condition is only
     * periodic for the mu it was found with. Using the wrong one turns a
     * closure test into a slow drift with no obvious cause. */
    {
        CHECK(fabs(mu - 1.215058560962404e-02) < 1e-17);

        double mu_from_gm = cr3bp_mu(398600.435436, 4902.800066);
        CHECK(fabs(mu - mu_from_gm) > 1e-10);
        CHECK(fabs(mu - mu_from_gm) < 1e-8);
    }

    /* Our Jacobi constant against JPL's, for the same states. Measured
     * agreement 0 to 4.9e-15 across all five orbits - machine precision on
     * quantities near 3. Two independent implementations of the same integral
     * agreeing this closely is strong evidence for both. */
    for (size_t i = 0; i < n_orbits; i++) {
        double ours = cr3bp_jacobi(orbit[i].s.r, orbit[i].s.v, mu);
        CHECK(fabs(ours - orbit[i].jacobi) < 1e-13);
    }

    /* Four of the five close to a few times 1e-11 at tol 1e-13.
     * Measured: 3.91e-11, 8.59e-11, 1.06e-11, 8.75e-12. */
    for (size_t i = 0; i < 4; i++) {
        CHECK(closure(&orbit[i], 1e-13) < 1e-9);
    }

    /* And the error is the integrator's, not the data's: it keeps falling
     * with the tolerance instead of reaching a floor. Measured for orbit 0:
     * 3.01e-10, 3.91e-11, 3.03e-12, 1.74e-13 at 1e-12, 1e-13, 1e-14, 1e-15. */
    {
        double loose = closure(&orbit[0], 1e-12);
        double tight = closure(&orbit[0], 1e-14);
        CHECK(tight < loose / 10.0);
    }

    /* Instability does not dominate closure over a single period, which is
     * worth stating because it is the opposite of what one expects. Orbit
     * 1151 amplifies perturbations by 594 per revolution and closes to
     * 3.24e-13; orbit 0 amplifies by 1.19 and closes to 3.03e-12 - the far
     * more unstable orbit closes better. Over one period the seed being
     * amplified is around 1e-16, so 594 times it is still below the
     * integrator's own error. What actually decides the difficulty is how
     * close the orbit passes to a primary.
     *
     * Those factors are the eigenvalues, not the catalogue's stability
     * column, which is (lambda + 1/lambda)/2 and so reads 297 and 1.015.
     * ROADMAP C3 and core/test/test_stability.c derive both; the amplification
     * per revolution is the eigenvalue, and this comment used to quote the
     * index in its place. */
    {
        CHECK(orbit[3].stability > 100.0);
        CHECK(orbit[0].stability < 1.1);
        CHECK(closure(&orbit[3], 1e-14) < closure(&orbit[0], 1e-14));
    }

    /* The fifth orbit is five orders of magnitude worse, and the reason is
     * geometry rather than anything numerical.
     *
     * Measured closest approach to the Moon: 0.00007 units, which is 29 km
     * from its centre - 1708 km below the surface. It is the terminal member
     * of the family, a near-rectilinear halo that in a point-mass model
     * simply passes through the Moon. The acceleration there is enormous and
     * the step controller has to work through a near-singular passage.
     *
     * Kept in the fixture rather than dropped: it is the hardest case
     * available and it is fully explained. The published period was checked
     * too, by scanning around it - no shift closes the orbit better, so the
     * catalogue value is right and the difficulty is real. */
    {
        double hard = closure(&orbit[4], 1e-13);
        CHECK(hard < 1e-4);
        CHECK(hard > closure(&orbit[0], 1e-13) * 100.0);

        double r_perilune = perilune(&orbit[4]);
        CHECK(r_perilune > 0.0);
        CHECK(r_perilune < 1e-3);
        CHECK(r_perilune * LUNIT_KM < MOON_RADIUS_KM);
    }

    /* The other four stay well clear. Measured closest approaches of 17657,
     * 7972, 36791 and 45894 km from the Moon's centre. */
    for (size_t i = 0; i < 4; i++) {
        double r = perilune(&orbit[i]);
        CHECK(r > 0.02);
        CHECK(r * LUNIT_KM > MOON_RADIUS_KM);
    }

    /* Halo orbits are symmetric about the xz-plane, so the catalogue's
     * initial conditions sit on a perpendicular crossing: y, vx and vz are
     * zero to within the precision the search converged to. This is the
     * structure differential correction will exploit in C2b. */
    for (size_t i = 0; i < 4; i++) {
        CHECK(fabs(orbit[i].s.r.y) < 1e-20);
        CHECK(fabs(orbit[i].s.v.x) < 1e-12);
        CHECK(fabs(orbit[i].s.v.z) < 1e-12);
        CHECK(fabs(orbit[i].s.r.z) > 0.05);   /* genuinely out of plane */
    }

    /* Loader behaviour. */
    {
        RefHalo tiny[2];
        size_t n = 0;
        CHECK(refdata_load_halo("data/jpl_halo/halo_l2_south.csv", tiny, 2, &n)
              == CORE_ERR_BUFFER_TOO_SMALL);
        CHECK(n == 2);

        double value = 0.0;
        CHECK(refdata_load_scalar("data/jpl_halo/nope.txt", &value)
              == CORE_ERR_INVALID_ARG);
        CHECK(refdata_load_halo(NULL, tiny, 2, &n) == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
