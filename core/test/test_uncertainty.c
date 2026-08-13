/* Covariance propagation (ROADMAP, "Відкрите питання" after C6).
 *
 * Four checks, each catching a different mistake:
 *
 *   - exact algebra (identity, a pure scaling) catches an index error in
 *     uncertainty_propagate itself, with no reference trajectory needed;
 *   - symmetry after a real STM catches an asymmetric bug that the exact
 *     cases are too simple to expose;
 *   - the marquee check compares the linear prediction against what the
 *     ACTUAL nonlinear dynamics do to a small cloud of states - the same
 *     idea ROADMAP C2b uses for the STM itself, one level up. If
 *     uncertainty_propagate disagrees with reality, the bug is here, not in
 *     stm_integrate (already checked in test_stm.c).
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "refdata.h"
#include "stm.h"
#include "test.h"
#include "uncertainty.h"

#include <math.h>
#include <string.h>

#define MAX_ORBITS 16
#define ORBIT 3 /* catalogue 1151, same member ex_trajectory.c flies */

static Dop853Config tight(void)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-14;
    cfg.max_steps = 20000000;
    return cfg;
}

static double *component(State *s, int axis)
{
    switch (axis) {
    case 0: return &s->r.x;
    case 1: return &s->r.y;
    case 2: return &s->r.z;
    case 3: return &s->v.x;
    case 4: return &s->v.y;
    default: return &s->v.z;
    }
}

int main(void)
{
    RefHalo orbit[MAX_ORBITS];
    size_t n_orbits;
    double mu;

    if (refdata_load_halo("data/jpl_halo/halo_l2_south.csv", orbit, MAX_ORBITS,
                          &n_orbits) != CORE_OK
        || refdata_load_scalar("data/jpl_halo/mu.txt", &mu) != CORE_OK
        || n_orbits <= ORBIT) {
        fprintf(stderr, "test_uncertainty: cannot read data/jpl_halo/\n");
        return EXIT_FAILURE;
    }

    Cr3bpCtx ctx = { mu };
    State s0 = orbit[ORBIT].s;
    s0.t = 0.0;
    double leg = orbit[ORBIT].period / 8.0; /* matches LEGS in ex_trajectory.c */

    /* ---- exact algebra: no trajectory involved ---- */
    {
        double p[STM_SIZE] = {
            4.0, 1.0, 0.5, 0.0, 0.0, 0.0,
            1.0, 3.0, 0.2, 0.0, 0.0, 0.0,
            0.5, 0.2, 2.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1e-2, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 1e-2, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 1e-2,
        };

        double identity[STM_SIZE];
        stm_identity(identity);

        double out[STM_SIZE];
        uncertainty_propagate(identity, p, out);
        for (int i = 0; i < STM_SIZE; i++) {
            CHECK_BITS_EQ(out[i], p[i]);
        }

        /* Phi = 2I: P' = Phi P Phi^T = 4P exactly (2 and 4 are exact in
         * binary, so this holds bit for bit, not just approximately). */
        double two_i[STM_SIZE];
        stm_identity(two_i);
        for (int i = 0; i < 6; i++) {
            two_i[i * 6 + i] = 2.0;
        }
        uncertainty_propagate(two_i, p, out);
        for (int i = 0; i < STM_SIZE; i++) {
            CHECK_BITS_EQ(out[i], 4.0 * p[i]);
        }

        /* A scale by a power of two is exact too. */
        double scaled[STM_SIZE];
        memcpy(scaled, p, sizeof scaled);
        uncertainty_scale(scaled, 0.25);
        for (int i = 0; i < STM_SIZE; i++) {
            CHECK_BITS_EQ(scaled[i], 0.25 * p[i]);
        }

        CHECK(uncertainty_symmetry_defect(p) == 0.0);
    }

    /* ---- symmetry survives a real STM, over one leg of a real orbit ---- */
    double phi[STM_SIZE];
    State end;
    {
        Dop853Config cfg = tight();
        Dop853State st;
        memset(&st, 0, sizeof st);
        CoreResult r = stm_integrate(accel_cr3bp_var, &ctx, &s0, s0.t + leg,
                                     &cfg, &st, &end, phi);
        CHECK(r == CORE_OK);

        double p0[STM_SIZE];
        memset(p0, 0, sizeof p0);
        p0[0 * 6 + 0] = 1e-10;
        p0[1 * 6 + 1] = 2e-10;
        p0[2 * 6 + 2] = 1.5e-10;
        p0[0 * 6 + 1] = p0[1 * 6 + 0] = 3e-11; /* off-diagonal on purpose */
        p0[3 * 6 + 3] = 1e-12;
        p0[4 * 6 + 4] = 1e-12;
        p0[5 * 6 + 5] = 1e-12;

        double p1[STM_SIZE];
        uncertainty_propagate(phi, p0, p1);

        double trace = p1[0] + p1[7] + p1[14] + p1[21] + p1[28] + p1[35];
        CHECK(uncertainty_symmetry_defect(p1) < 1e-12 * trace);
    }

    /* ---- marquee: linear prediction vs the actual nonlinear spread ---- */
    {
        /* Dimensionless CR3BP units; 1e-6 of the Earth-Moon distance is
         * about 384 m, small enough that one leg (~1/8 period) stays deep
         * in the regime where the linearisation holds. */
        const double eps_r = 1e-6;
        const double eps_v = 1e-6;
        const double axis_eps[6] = { eps_r, eps_r, eps_r, eps_v, eps_v, eps_v };

        double p0[STM_SIZE];
        memset(p0, 0, sizeof p0);
        for (int i = 0; i < 3; i++) {
            p0[i * 6 + i] = eps_r * eps_r;
        }
        for (int i = 3; i < 6; i++) {
            p0[i * 6 + i] = eps_v * eps_v;
        }

        double p_linear[STM_SIZE];
        uncertainty_propagate(phi, p0, p_linear);

        /* Twelve deterministic sigma points, +/- eps along each axis -
         * the same finite-difference idea test_stm.c uses for the STM
         * itself, not a random sample, so the result is reproducible. */
        double sum_sq[6];
        memset(sum_sq, 0, sizeof sum_sq);

        for (int axis = 0; axis < 6 && core_test_failures == 0; axis++) {
            for (int sign = -1; sign <= 1; sign += 2) {
                State perturbed = s0;
                *component(&perturbed, axis) += (double)sign * axis_eps[axis];

                Dop853Config cfg = tight();
                Dop853State st;
                memset(&st, 0, sizeof st);
                State out;
                CoreResult r = dop853_integrate(accel_cr3bp, &ctx, &perturbed,
                                                s0.t + leg, &cfg, &st, &out);
                CHECK(r == CORE_OK);

                double dev[6] = {
                    out.r.x - end.r.x, out.r.y - end.r.y, out.r.z - end.r.z,
                    out.v.x - end.v.x, out.v.y - end.v.y, out.v.z - end.v.z,
                };
                for (int k = 0; k < 6; k++) {
                    sum_sq[k] += dev[k] * dev[k];
                }
            }
        }

        /* Sample variance per output axis k, reconstructed from the sigma
         * points: each input axis j contributes a +/-eps_j pair, and to
         * first order dev_k from that pair is +/-Phi[k,j]*eps_j, so the pair
         * alone sums to 2*Phi[k,j]^2*eps_j^2. Summing that over all six
         * input axes (which is exactly what the accumulation loop above
         * does into sum_sq[k]) gives 2 * sum_j Phi[k,j]^2 eps_j^2 - twice
         * the k-th diagonal of Phi P0 Phi^T for a diagonal P0. So the
         * divisor is 2, not the total point count (12): that would count
         * sigma points, but the estimator sums over six independent
         * axis-pairs, each already complete in itself. */
        for (int k = 0; k < 6; k++) {
            double sample_var = sum_sq[k] / 2.0;
            double linear_var = p_linear[k * 6 + k];
            double rel = (sample_var - linear_var) / linear_var;
            if (rel < 0.0) {
                rel = -rel;
            }
            /* Measured: worst axis 2.9e-10. Four orders of margin, not a
             * loose bound - this catches a real regression in either
             * uncertainty_propagate or accel_cr3bp_var's Jacobian, not just
             * "something is very wrong". */
            CHECK(rel < 1e-6);
        }
    }

    return TEST_RESULT();
}
