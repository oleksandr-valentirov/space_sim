/* RK4 against the two-body problem (ROADMAP B3).
 *
 * The two-body problem is the only case with a closed-form answer, which
 * makes it the one place the integrator can be measured against truth rather
 * than against another integrator. Every number in here was measured before
 * it was asserted. */

#include "integrator.h"
#include "test.h"

#include <math.h>

/* GM of the Earth from data/horizons/gm.csv, in m^3/s^2. Hard-coded here
 * rather than loaded, so this test says nothing about file handling. */
#define MU_EARTH 3.98600435436e14

#define R0 7.0e6

static State circular_orbit(double *period_out, TwoBodyCtx *ctx)
{
    ctx->mu = MU_EARTH;
    double v_circ = sqrt(MU_EARTH / R0);
    State s = { { R0, 0.0, 0.0 }, { 0.0, v_circ, 0.0 }, 0.0 };
    *period_out = two_body_period(s.r, s.v, MU_EARTH);
    return s;
}

static double position_error_after_one_orbit(const State *s0, double period,
                                             TwoBodyCtx *ctx, int n_steps)
{
    State s1;
    CoreResult r = rk4_integrate(accel_two_body, ctx, s0, period,
                                 period / (double)n_steps, &s1);
    CHECK(r == CORE_OK);
    return vec3_distance(s1.r, s0->r);
}

int main(void)
{
    TwoBodyCtx ctx;
    double period;
    State s0 = circular_orbit(&period, &ctx);

    /* The setup itself, before anything is integrated. */
    {
        CHECK(fabs(vec3_norm(s0.v) - 7546.053) < 1e-3);
        CHECK(fabs(period - 5828.516684) < 1e-5);

        /* Angular momentum points along +z for a prograde orbit in the
         * xy-plane. A sign error in the cross product would flip it. */
        Vec3d h = two_body_angular_momentum(s0.r, s0.v);
        CHECK(h.z > 0.0);
        CHECK_BITS_EQ(h.x, 0.0);
        CHECK_BITS_EQ(h.y, 0.0);

        /* Bound orbit: energy is negative, and equals -mu/(2a) with a = R0. */
        double e = two_body_energy(s0.r, s0.v, MU_EARTH);
        CHECK(e < 0.0);
        CHECK(fabs(e - (-MU_EARTH / (2.0 * R0))) < 1e-6);

        /* An escaping state has no period rather than a NaN one. */
        State escape = { { R0, 0.0, 0.0 }, { 0.0, 2.0 * vec3_norm(s0.v), 0.0 }, 0.0 };
        CHECK_BITS_EQ(two_body_period(escape.r, escape.v, MU_EARTH), 0.0);
    }

    /* Order of the method. This is the test that matters: a wrong coefficient
     * in the Butcher tableau still produces a plausible orbit, but it does
     * not produce fourth-order convergence.
     *
     * Measured, one full orbit: 6.69e-2, 4.01e-3, 2.45e-4 m at N = 400, 800,
     * 1600, for ratios of 17.32, 16.69 and 16.34 against the 16 that fourth
     * order predicts. Below about h = 1 s the ratio collapses towards 1 as
     * rounding takes over at a floor near 3e-7 m, so the test stays above
     * that regime. */
    {
        double e400 = position_error_after_one_orbit(&s0, period, &ctx, 400);
        double e800 = position_error_after_one_orbit(&s0, period, &ctx, 800);
        double e1600 = position_error_after_one_orbit(&s0, period, &ctx, 1600);

        CHECK(e400 < 0.1);
        CHECK(e800 < 1e-2);
        CHECK(e1600 < 1e-3);

        CHECK(e400 / e800 > 14.0 && e400 / e800 < 20.0);
        CHECK(e800 / e1600 > 14.0 && e800 / e1600 < 20.0);
    }

    /* Energy drift. Measured 1.69e-13 relative over one orbit at N=1600. */
    {
        State s1;
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, period,
                            period / 1600.0, &s1) == CORE_OK);

        double e_before = two_body_energy(s0.r, s0.v, MU_EARTH);
        double e_after = two_body_energy(s1.r, s1.v, MU_EARTH);
        CHECK(fabs((e_after - e_before) / e_before) < 1e-12);

        /* Angular momentum is conserved separately, and fails differently: a
         * sign error in the force can leave energy looking sane while this
         * collapses. */
        Vec3d h_before = two_body_angular_momentum(s0.r, s0.v);
        Vec3d h_after = two_body_angular_momentum(s1.r, s1.v);
        CHECK(fabs(vec3_norm(h_after) / vec3_norm(h_before) - 1.0) < 1e-12);
    }

    /* Quarter orbit lands where geometry says it must, not merely back where
     * it started. A full-period test alone would pass for an orbit traversed
     * in the wrong direction or at the wrong rate. */
    {
        State q;
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 0.25 * period,
                            period / 1600.0, &q) == CORE_OK);
        CHECK(fabs(q.r.x) < 1e-3);
        CHECK(fabs(q.r.y - R0) < 1e-3);
        CHECK(fabs(q.r.z) < 1e-9);
        CHECK(fabs(vec3_norm(q.r) - R0) < 1e-3);
    }

    /* Kepler's third law, as an independent check of the whole setup: an
     * orbit with periapsis R0 and 1.6 times the circular kinetic energy has
     * a = 2.5*R0, so its period must be 2.5^1.5 times the circular one.
     * Measured ratio 3.9528, and 2.5^1.5 = 3.952847... */
    {
        double v_circ = sqrt(MU_EARTH / R0);
        State e0 = { { R0, 0.0, 0.0 },
                     { 0.0, v_circ * sqrt(1.6), 0.0 }, 0.0 };
        double period_e = two_body_period(e0.r, e0.v, MU_EARTH);

        double expected = pow(2.5, 1.5);
        CHECK(fabs(period_e / period / expected - 1.0) < 1e-12);

        /* And the eccentric orbit closes too. Measured 1.99e-3 m at N=6400;
         * it needs more steps than the circular case because periapsis is
         * where a fixed step is least adequate - which is exactly why the
         * runtime integrator is adaptive. */
        State e1;
        CHECK(rk4_integrate(accel_two_body, &ctx, &e0, period_e,
                            period_e / 6400.0, &e1) == CORE_OK);
        CHECK(vec3_distance(e1.r, e0.r) < 1e-2);
    }

    /* Reversibility: three orbits out, three back. Catches sign handling in
     * the step, and it is the same check B4 will apply over ten years.
     *
     * Measured round-trip position error 9.94e-2, 3.11e-3, 9.72e-5, 1.23e-6 m
     * at N = 400, 800, 1600, 3200 per orbit - ratios of 32, not 16. The
     * leading fourth-order term cancels on the way back, leaving fifth-order
     * behaviour. Not exact cancellation: classical RK4 is not self-adjoint,
     * so the round trip does not return to the start exactly. The upside is
     * that this test is more sensitive than one-way integration, not less.
     *
     * Velocity error tracks position error by the factor v/r = 1.08e-3, as
     * it must for a circular orbit: 1.05e-7 m/s against 9.72e-5 m at N=1600. */
    {
        State forward, back;
        double h = period / 1600.0;
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 3.0 * period, h,
                            &forward) == CORE_OK);
        CHECK(rk4_integrate(accel_two_body, &ctx, &forward, 0.0, h,
                            &back) == CORE_OK);

        double dr = vec3_distance(back.r, s0.r);
        CHECK(dr < 1e-3);
        CHECK(vec3_norm(vec3_sub(back.v, s0.v)) < 1e-6);
        CHECK_BITS_EQ(back.t, 0.0);

        /* The fifth-order round-trip convergence, asserted rather than just
         * observed: it is a much tighter statement about the step than any
         * single error bound. */
        State f2, b2;
        double h2 = period / 800.0;
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 3.0 * period, h2,
                            &f2) == CORE_OK);
        CHECK(rk4_integrate(accel_two_body, &ctx, &f2, 0.0, h2, &b2) == CORE_OK);

        double ratio = vec3_distance(b2.r, s0.r) / dr;
        CHECK(ratio > 24.0 && ratio < 40.0);
    }

    /* Step bookkeeping. */
    {
        State out;
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 0.0, 1.0, &out) == CORE_OK);
        CHECK(vec3_equal_bits(out.r, s0.r));
        CHECK_BITS_EQ(out.t, 0.0);

        /* The end time is hit exactly even when it is not a multiple of h. */
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 1234.5, 100.0, &out) == CORE_OK);
        CHECK_BITS_EQ(out.t, 1234.5);

        CHECK(rk4_integrate(NULL, &ctx, &s0, 1.0, 1.0, &out) == CORE_ERR_INVALID_ARG);
        CHECK(rk4_integrate(accel_two_body, &ctx, &s0, 1.0, 0.0, &out)
              == CORE_ERR_INVALID_ARG);
        CHECK(rk4_step(accel_two_body, &ctx, &s0, 1.0, NULL) == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
