/* Instability of halo orbits, measured (ROADMAP C3).
 *
 * The point of this step is to stop treating "the orbit fell apart" as a
 * symptom of a bug and start treating it as a number. Once the monodromy
 * matrix says a perturbation is multiplied by 150 per revolution, an orbit
 * that survives four revolutions from a seed at 1e-12 is behaving exactly as
 * it should, and there is no bug to look for.
 *
 * So this file does two separate things: it computes the eigenvalue moduli,
 * and it then measures the actual growth of an actual perturbation and checks
 * the two agree. Either alone would be much weaker.
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "refdata.h"
#include "stm.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16

static RefHalo orbit[MAX_ORBITS];
static size_t n_orbits;
static double mu;
static Cr3bpCtx ctx;

static Dop853Config tight(void)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-14;
    cfg.max_steps = 20000000;
    return cfg;
}

static CoreResult monodromy(const RefHalo *h, double m[STM_SIZE])
{
    Dop853Config cfg = tight();
    Dop853State st;
    memset(&st, 0, sizeof st);
    State end;

    return stm_integrate(accel_cr3bp_var, &ctx, &h->s, h->period, &cfg, &st,
                         &end, m);
}

static CoreResult propagate(const State *s, double t, State *out)
{
    Dop853Config cfg = tight();
    Dop853State st;
    memset(&st, 0, sizeof st);

    return dop853_integrate(accel_cr3bp, &ctx, s, t, &cfg, &st, out);
}

/* Size of the separation between a trajectory and a displaced copy of it,
 * after n revolutions. */
static double separation(const RefHalo *h, double eps, int revolutions)
{
    State displaced = h->s;
    displaced.r.x += eps;
    displaced.r.z += eps;
    displaced.v.y += eps;

    State a, b;
    double t = h->period * (double)revolutions;
    if (propagate(&h->s, t, &a) != CORE_OK ||
        propagate(&displaced, t, &b) != CORE_OK) {
        return -1.0;
    }

    return sqrt(vec3_norm_sq(vec3_sub(b.r, a.r))
                + vec3_norm_sq(vec3_sub(b.v, a.v)));
}

int main(void)
{
    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit,
                          MAX_ORBITS, &n_orbits) != CORE_OK ||
        refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }
    ctx.mu = mu;

    /* The catalogue publishes a stability index for every orbit, and
     * reproducing it is the check that the whole reduction - traces, Newton's
     * identities, dividing out the unit pair, the quadratic - is right.
     * Measured agreement: exact to every digit published for the first three
     * orbits, and to 4e-5 relative for orbit 1151, whose monodromy entries
     * reach 594 and whose integration error therefore shows.
     *
     * The index is max|mu|/2, not the eigenvalue. That distinction is the
     * subject of the next block. */
    for (size_t i = 0; i < 4; i++) {
        double m[STM_SIZE];
        CHECK(monodromy(&orbit[i], m) == CORE_OK);

        StmStability s;
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);
        CHECK(s.real_pair == 1);

        double biggest = s.index[0] > s.index[1] ? s.index[0] : s.index[1];
        CHECK(fabs(biggest - orbit[i].stability)
              < 1e-4 * orbit[i].stability);

        /* The unit eigenvalue pair really is there. Measured residual 5e-10
         * to 2e-8, which is the monodromy's own accuracy rather than a
         * property of the orbit. */
        CHECK(fabs(s.unit_pair_residual) < 1e-6);

        /* lambda and 1/lambda average to the index, by construction. */
        for (int k = 0; k < 2; k++) {
            if (fabs(s.invariant[k]) <= 2.0) {
                continue;
            }
            double lambda = (fabs(s.invariant[k])
                             + sqrt(s.invariant[k] * s.invariant[k] - 4.0))
                            / 2.0;
            CHECK(fabs((lambda + 1.0 / lambda) / 2.0 - s.index[k]) < 1e-9);
        }
    }

    /* The index is not the growth rate, and the difference is not small.
     *
     * Orbit 1151 has index 297 and eigenvalue 594; orbit 0 has index 1.015
     * and eigenvalue 1.19. Reading the catalogue column as "how much a
     * perturbation grows per revolution" understates it by a factor of two
     * for the unstable orbits and by twelve for the nearly stable one. */
    {
        double m[STM_SIZE];
        StmStability s;

        CHECK(monodromy(&orbit[3], m) == CORE_OK);
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);
        CHECK(fabs(s.lambda_max - 594.13) < 0.1);
        CHECK(fabs(orbit[3].stability - 297.07) < 0.01);

        CHECK(monodromy(&orbit[0], m) == CORE_OK);
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);
        CHECK(fabs(s.lambda_max - 1.1905) < 1e-3);
        CHECK(fabs(orbit[0].stability - 1.0152) < 1e-3);
    }

    /* And now the measurement the whole step exists for: displace the orbit
     * and watch it leave.
     *
     * The ratio of separations one revolution apart converges on lambda_max.
     * Measured for orbit 767: 149.65 and 149.71 against a predicted 149.717,
     * and for orbit 1151: 594.47 and 594.08 against 594.134. The seed is
     * 1e-12 so that even after the growth the motion is still linear; at 1e-10
     * the last revolution of orbit 1151 has already left the linear regime and
     * the agreement degrades to a percent, which is itself worth knowing. */
    {
        const double eps = 1e-12;
        size_t unstable[2] = { 2, 3 };
        int first[2] = { 2, 1 };

        for (int u = 0; u < 2; u++) {
            size_t i = unstable[u];

            double m[STM_SIZE];
            StmStability s;
            CHECK(monodromy(&orbit[i], m) == CORE_OK);
            CHECK(stm_monodromy_stability(m, &s) == CORE_OK);

            for (int rev = first[u]; rev < first[u] + 2; rev++) {
                double a = separation(&orbit[i], eps, rev);
                double b = separation(&orbit[i], eps, rev + 1);
                CHECK(a > 0.0);
                CHECK(b > 0.0);
                CHECK(fabs(b / a / s.lambda_max - 1.0) < 2e-3);
            }
        }
    }

    /* Time to departure, which is what a mission planner actually asks. From
     * a displacement of eps, the number of revolutions before the separation
     * reaches a given size is ln(size/eps)/ln(lambda) - and since that is a
     * prediction rather than a fit, it is worth checking against the
     * integrator. For orbit 767 growing from 1e-12 to 1e-5, the prediction is
     * 4.02 revolutions; the measured separation is below the threshold at 4
     * revolutions and above it at 5. */
    {
        const double eps = 1e-12;
        const double threshold = 1e-5;

        double m[STM_SIZE];
        StmStability s;
        CHECK(monodromy(&orbit[2], m) == CORE_OK);
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);

        double predicted = log(threshold / separation(&orbit[2], eps, 0))
                           / log(s.lambda_max);
        CHECK(predicted > 3.0);
        CHECK(predicted < 5.0);

        int floor_rev = (int)predicted;
        CHECK(separation(&orbit[2], eps, floor_rev) < threshold);
        CHECK(separation(&orbit[2], eps, floor_rev + 1) > threshold);
    }

    /* The weakly unstable orbits are the interesting case, and they do NOT
     * depart exponentially over any timescale one would watch.
     *
     * Orbit 0 has lambda 1.19, so six revolutions should multiply a
     * perturbation by 2.85. Measured: 3823. The growth is not the unstable
     * eigenvalue at all, it is the unit eigenvalue pair, which is defective -
     * one eigenvector for two eigenvalues - so a generic displacement drifts
     * along the family and around the orbit linearly in time, and that
     * swamps a factor of 1.19 for many revolutions.
     *
     * This is the finding that stops a bug hunt. "The nearly stable halo
     * drifted off faster than the stability index predicts" is not a broken
     * integrator, it is what a Jordan block does. */
    {
        const double eps = 1e-10;

        double m[STM_SIZE];
        StmStability s;
        CHECK(monodromy(&orbit[0], m) == CORE_OK);
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);

        double start = separation(&orbit[0], eps, 0);
        double after = separation(&orbit[0], eps, 6);
        CHECK(start > 0.0);
        CHECK(after > 0.0);

        double observed = after / start;
        double exponential = pow(s.lambda_max, 6.0);
        CHECK(exponential < 3.0);
        CHECK(observed > 100.0 * exponential);

        /* And the excess grows no faster than a low power of the revolution
         * count, which is what distinguishes a defective eigenvalue from an
         * exponential one that was mis-measured. */
        double half = separation(&orbit[0], eps, 3) / start;
        CHECK(observed / half < 20.0);
    }

    /* The near-rectilinear member reports its own unreliability. Its
     * integration is poor for reasons core/test/test_halo.c sets out - it
     * passes 1708 km below the lunar surface - and the unit-pair residual
     * says so: 2e-2 against 5e-10 for the well-behaved orbits. A caller that
     * checks this number is not fooled by the stability it would otherwise
     * read off. */
    {
        double m[STM_SIZE];
        StmStability s;
        CHECK(monodromy(&orbit[4], m) == CORE_OK);
        CHECK(stm_monodromy_stability(m, &s) == CORE_OK);
        CHECK(fabs(s.unit_pair_residual) > 1e-3);
    }

    /* The identity is the monodromy of nothing moving: every eigenvalue is
     * one, so both invariants are exactly 2 and the residual vanishes. */
    {
        double eye[STM_SIZE];
        stm_identity(eye);

        StmStability s;
        CHECK(stm_monodromy_stability(eye, &s) == CORE_OK);
        CHECK(s.real_pair == 1);
        CHECK_BITS_EQ(s.invariant[0], 2.0);
        CHECK_BITS_EQ(s.invariant[1], 2.0);
        CHECK_BITS_EQ(s.lambda_max, 1.0);
        CHECK_BITS_EQ(s.unit_pair_residual, 0.0);

        CHECK(stm_monodromy_stability(NULL, &s) == CORE_ERR_INVALID_ARG);
        CHECK(stm_monodromy_stability(eye, NULL) == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
