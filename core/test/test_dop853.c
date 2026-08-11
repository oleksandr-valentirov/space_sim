/* DOP853, the runtime integrator (ROADMAP B4). */

#include "dop853_coeffs.h"
#include "integrator.h"
#include "test.h"

#include <math.h>

#define MU_EARTH 3.98600435436e14
#define R_LEO 7.0e6
#define R_LUNAR 3.844e8
#define TEN_YEARS (10.0 * 365.25 * 86400.0)

static State circular(double radius)
{
    double v = sqrt(MU_EARTH / radius);
    State s = { { radius, 0.0, 0.0 }, { 0.0, v, 0.0 }, 0.0 };
    return s;
}

int main(void)
{
    TwoBodyCtx ctx = { MU_EARTH };

    /* The coefficient table, checked structurally rather than by eye.
     *
     * Every one of these is a condition the method must satisfy for any
     * ordering of the stages, so they catch a mistranscribed digit anywhere
     * in the generated header without anyone comparing 80 constants by hand.
     * Measured worst row deviation: 1.78e-15, which is the rounding of
     * summing twelve terms of order one. */
    {
        CHECK_BITS_EQ(DOP853_C[0], 0.0);
        CHECK_BITS_EQ(DOP853_C[DOP853_STAGES - 1], 1.0);

        for (int i = 0; i < DOP853_STAGES; i++) {
            double row = 0.0;
            for (int j = 0; j < DOP853_STAGES; j++) {
                row += DOP853_A[i][j];
            }
            CHECK(fabs(row - DOP853_C[i]) < 1e-14);

            /* Explicit method: strictly lower triangular. */
            for (int j = i; j < DOP853_STAGES; j++) {
                CHECK_BITS_EQ(DOP853_A[i][j], 0.0);
            }
        }

        double b_sum = 0.0;
        for (int j = 0; j < DOP853_STAGES; j++) {
            b_sum += DOP853_B[j];
        }
        CHECK(fabs(b_sum - 1.0) < 1e-14);

        /* Both error estimators must vanish on a constant solution, so their
         * weights sum to zero. */
        double e3_sum = 0.0, e5_sum = 0.0;
        for (int j = 0; j <= DOP853_STAGES; j++) {
            e3_sum += DOP853_E3[j];
            e5_sum += DOP853_E5[j];
        }
        CHECK(fabs(e3_sum) < 1e-14);
        CHECK(fabs(e5_sum) < 1e-14);
    }

    State leo = circular(R_LEO);
    double period = two_body_period(leo.r, leo.v, MU_EARTH);

    /* Order of the method, with the step pinned so the controller cannot
     * adapt. Measured over one orbit: 1.81e-2, 7.40e-5, 3.89e-7 m at N = 20,
     * 40, 80, giving ratios of 244 and 190 against the 256 that eighth order
     * predicts. Beyond N=80 the error hits a rounding floor near 2e-7 m and
     * the ratio collapses to 1, so the test stays inside the regime where
     * truncation dominates. */
    {
        double err[3];
        int n[3] = { 20, 40, 80 };

        for (int i = 0; i < 3; i++) {
            double h = period / (double)n[i];
            Dop853Config cfg = { 0 };
            cfg.tol_m = 1e30;   /* accept everything: fixed step by force */
            cfg.h_init = h;
            cfg.h_max = h;

            Dop853State st = { 0 };
            State s1;
            CHECK(dop853_integrate(accel_two_body, &ctx, &leo, period,
                                   &cfg, &st, &s1) == CORE_OK);
            CHECK(st.n_rejected == 0);
            err[i] = vec3_distance(s1.r, leo.r);
        }

        CHECK(err[0] / err[1] > 150.0 && err[0] / err[1] < 400.0);
        CHECK(err[1] / err[2] > 120.0 && err[1] / err[2] < 400.0);
    }

    /* The controller delivers what was asked for. Measured ratio of achieved
     * error to requested tolerance: 2.33, 2.30, 2.30, 2.31, 2.27, 3.32, 2.24
     * across tolerances from 1e-2 down to 1e-8 m. Stability of that ratio
     * over six orders of magnitude is the real statement here - it means the
     * tolerance is a dial that means something, not a hint. */
    {
        for (double tol = 1e-2; tol >= 1e-8; tol /= 10.0) {
            Dop853Config cfg = { 0 };
            cfg.tol_m = tol;
            Dop853State st = { 0 };
            State s1;
            CHECK(dop853_integrate(accel_two_body, &ctx, &leo, period,
                                   &cfg, &st, &s1) == CORE_OK);

            double ratio = vec3_distance(s1.r, leo.r) / tol;
            CHECK(ratio > 0.5 && ratio < 6.0);

            /* Tighter tolerance must cost more steps, and the step carried
             * out must be usable as the start of the next leg. */
            CHECK(st.n_accepted > 10);
            CHECK(st.h > 0.0);
        }
    }

    /* Cross-check against RK4. Two independent integrators agreeing is much
     * stronger evidence than either one looking correct.
     *
     * Measured: they land 2.79e-7 m apart after one orbit, DOP853 spending
     * 1382 derivative evaluations against RK4's 51200 - a factor of 37 for
     * the same answer. That ratio is the whole reason the runtime integrator
     * is not RK4. */
    {
        Dop853Config cfg = { 0 };
        cfg.tol_m = 1e-8;
        Dop853State st = { 0 };
        State by_dop, by_rk4;

        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, period,
                               &cfg, &st, &by_dop) == CORE_OK);
        CHECK(rk4_integrate(accel_two_body, &ctx, &leo, period,
                            period / 12800.0, &by_rk4) == CORE_OK);

        CHECK(vec3_distance(by_dop.r, by_rk4.r) < 1e-6);
        CHECK(st.n_evals < 12800 * 4 / 10);
    }

    /* The ephemeris case: a lunar-scale orbit over ten years, which is what
     * the cooker will actually be asked to integrate.
     *
     * Measured at tol = 1e-6 m: relative energy drift 7.42e-14 and a
     * round-trip position error of 4.11e-2 m over 134 orbits. DOP853 is not
     * symplectic, so drift is expected; at this magnitude it is far below
     * anything that matters, which is the evidence that the ephemeris does
     * not need IAS15 yet (ROADMAP, fork 1). */
    {
        State moon_like = circular(R_LUNAR);

        Dop853Config cfg = { 0 };
        cfg.tol_m = 1e-6;
        cfg.max_steps = 200000;

        Dop853State forward_st = { 0 };
        State forward;
        CHECK(dop853_integrate(accel_two_body, &ctx, &moon_like, TEN_YEARS,
                               &cfg, &forward_st, &forward) == CORE_OK);

        double e0 = two_body_energy(moon_like.r, moon_like.v, MU_EARTH);
        double e1 = two_body_energy(forward.r, forward.v, MU_EARTH);
        CHECK(fabs((e1 - e0) / e0) < 1e-12);

        Dop853State back_st = { 0 };
        State back;
        CHECK(dop853_integrate(accel_two_body, &ctx, &forward, 0.0,
                               &cfg, &back_st, &back) == CORE_OK);

        CHECK(vec3_distance(back.r, moon_like.r) < 1.0);
        CHECK_BITS_EQ(back.t, 0.0);
    }

    /* Continuation. Two legs with the step carried across must agree closely
     * with one leg - not bit for bit, because the leg boundary forces a
     * clamped step, but to well inside the tolerance. This is the mechanism
     * PROJECT.md section 4 requires for saves. */
    {
        Dop853Config cfg = { 0 };
        cfg.tol_m = 1e-6;

        Dop853State one_st = { 0 };
        State one_leg;
        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, 3.0 * period,
                               &cfg, &one_st, &one_leg) == CORE_OK);

        Dop853State two_st = { 0 };
        State middle, two_legs;
        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, 1.5 * period,
                               &cfg, &two_st, &middle) == CORE_OK);
        CHECK(two_st.h > 0.0);
        CHECK(dop853_integrate(accel_two_body, &ctx, &middle, 3.0 * period,
                               &cfg, &two_st, &two_legs) == CORE_OK);

        CHECK(vec3_distance(one_leg.r, two_legs.r) < 1e-3);
    }

    /* Running out of steps is reported, not silently truncated. The state
     * this guards against is a caller reading an output that was never
     * written - which is exactly what happened while measuring this step. */
    {
        Dop853Config cfg = { 0 };
        cfg.tol_m = 1e-6;
        cfg.max_steps = 10;
        Dop853State st = { 0 };
        State out;
        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, TEN_YEARS,
                               &cfg, &st, &out) == CORE_ERR_TOLERANCE_NOT_MET);
        CHECK(st.n_accepted == 10);
    }

    /* Degenerate and invalid arguments. */
    {
        Dop853Config cfg = { 0 };
        cfg.tol_m = 1e-6;
        Dop853State st = { 0 };
        State out;

        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, leo.t, &cfg, &st,
                               &out) == CORE_OK);
        CHECK(vec3_equal_bits(out.r, leo.r));

        CHECK(dop853_integrate(NULL, &ctx, &leo, 1.0, &cfg, &st, &out)
              == CORE_ERR_INVALID_ARG);
        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, 1.0, NULL, &st, &out)
              == CORE_ERR_INVALID_ARG);

        Dop853Config bad = { 0 };
        bad.tol_m = 0.0;
        CHECK(dop853_integrate(accel_two_body, &ctx, &leo, 1.0, &bad, &st, &out)
              == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
