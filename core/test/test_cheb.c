#include "cheb.h"
#include "cheb_fit.h"
#include "test.h"

#include <math.h>

/* Independent evaluation by the T_j recurrence, used only to cross-check
 * Clenshaw. Two implementations of the same sum that agree are far better
 * evidence than one implementation that looks right. */
static double eval_direct(const double *c, size_t n, double a, double b, double x)
{
    double s = (2.0 * x - (a + b)) / (b - a);
    double t_prev = 1.0;
    double t = s;
    double sum = c[0];

    if (n > 1) {
        sum += c[1] * t;
    }
    for (size_t j = 2; j < n; j++) {
        double t_next = 2.0 * s * t - t_prev;
        sum += c[j] * t_next;
        t_prev = t;
        t = t_next;
    }
    return sum;
}

static double f_cubic(double x, void *ctx)
{
    (void)ctx;
    return 2.0 * x * x * x - 3.0 * x * x + 0.5 * x - 7.0;
}

static double f_exp(double x, void *ctx)
{
    (void)ctx;
    return exp(x);
}

static double f_runge(double x, void *ctx)
{
    (void)ctx;
    return 1.0 / (1.0 + 25.0 * x * x);
}

/* Largest error over a dense sweep of the interval. Sampling at points that
 * are not the fitting nodes matters: at the nodes the fit is exact by
 * construction, so testing there would measure nothing. */
static double max_error(const double *c, size_t n, double a, double b,
                        ChebFunc f, void *ctx, int samples)
{
    double worst = 0.0;
    for (int i = 0; i <= samples; i++) {
        double x = a + (b - a) * ((double)i / (double)samples);
        double e = fabs(cheb_eval(c, n, a, b, x) - f(x, ctx));
        if (e > worst) {
            worst = e;
        }
    }
    return worst;
}

int main(void)
{
    double c[CHEB_FIT_MAX_N];

    /* T_0 = 1, T_1 = s, T_2 = 2s^2 - 1, evaluated through the public entry
     * point so the domain mapping is exercised too. */
    {
        double c0[1] = { 3.0 };
        CHECK_BITS_EQ(cheb_eval(c0, 1, -1.0, 1.0, 0.25), 3.0);

        double c1[2] = { 0.0, 1.0 };
        CHECK_BITS_EQ(cheb_eval(c1, 2, -1.0, 1.0, 0.25), 0.25);

        double c2[3] = { 0.0, 0.0, 1.0 };
        CHECK_BITS_EQ(cheb_eval(c2, 3, -1.0, 1.0, 0.5), 2.0 * 0.25 - 1.0);

        /* Mapped domain: the endpoints must land on s = -1 and s = +1. */
        CHECK_BITS_EQ(cheb_eval(c1, 2, 10.0, 20.0, 10.0), -1.0);
        CHECK_BITS_EQ(cheb_eval(c1, 2, 10.0, 20.0, 20.0), 1.0);
        CHECK_BITS_EQ(cheb_eval(c1, 2, 10.0, 20.0, 15.0), 0.0);

        CHECK_BITS_EQ(cheb_eval(c0, 0, -1.0, 1.0, 0.5), 0.0);
    }

    /* A cubic is in the span of the first four Chebyshev polynomials, so the
     * fit is exact up to rounding no matter how many coefficients are asked
     * for. This is the test that catches a wrong node formula or a misplaced
     * factor of two: an approximation that is merely good would still pass a
     * loose tolerance, but only a correct one is exact here. */
    {
        CHECK(cheb_fit(f_cubic, NULL, -1.0, 1.0, c, 8) == CORE_OK);
        CHECK(max_error(c, 8, -1.0, 1.0, f_cubic, NULL, 500) < 1e-14);

        /* Coefficients above the polynomial degree must vanish. */
        for (size_t j = 4; j < 8; j++) {
            CHECK(fabs(c[j]) < 1e-14);
        }

        /* And on a shifted, scaled interval. */
        CHECK(cheb_fit(f_cubic, NULL, 3.0, 11.0, c, 8) == CORE_OK);
        CHECK(max_error(c, 8, 3.0, 11.0, f_cubic, NULL, 500) < 1e-11);
    }

    /* Convergence: for a smooth function the error must fall fast and reach
     * machine precision. Recorded as measured values rather than as a vague
     * "gets better". */
    {
        CHECK(cheb_fit(f_exp, NULL, 0.0, 2.0, c, 4) == CORE_OK);
        double e4 = max_error(c, 4, 0.0, 2.0, f_exp, NULL, 500);

        CHECK(cheb_fit(f_exp, NULL, 0.0, 2.0, c, 8) == CORE_OK);
        double e8 = max_error(c, 8, 0.0, 2.0, f_exp, NULL, 500);

        CHECK(cheb_fit(f_exp, NULL, 0.0, 2.0, c, 16) == CORE_OK);
        double e16 = max_error(c, 16, 0.0, 2.0, f_exp, NULL, 500);

        /* Measured: 1.81e-2, 6.05e-7, 5.33e-15. Saturation sets in at n=14
         * around 7e-15, which is machine precision relative to exp(2). The
         * bounds sit just above the measurements, so a real regression trips
         * them but ordinary rounding noise does not. */
        CHECK(e4 < 3e-2);
        CHECK(e8 < 1e-6);
        CHECK(e16 < 1e-14);
        CHECK(e8 < e4 && e16 < e8);
    }

    /* Runge's function: the classic case where equally spaced interpolation
     * diverges. Chebyshev nodes are the whole reason for this node choice, so
     * the property is asserted rather than assumed.
     *
     * Measured: 8.31e-2, 3.47e-3, 1.44e-4, 6.00e-6 at n = 16, 32, 48, 64 -
     * a factor of about 24 per 16 coefficients. Theory predicts rho^16 with
     * rho = 1/5 + sqrt(1 + 1/25) = 1.2198 from the poles at +-i/5, which is
     * 24.0. The agreement is what actually confirms the nodes are right; the
     * absolute bound below would pass for a merely decent fit too. */
    {
        CHECK(cheb_fit(f_runge, NULL, -1.0, 1.0, c, 64) == CORE_OK);
        double e64 = max_error(c, 64, -1.0, 1.0, f_runge, NULL, 2000);
        CHECK(e64 < 1e-5);

        CHECK(cheb_fit(f_runge, NULL, -1.0, 1.0, c, 48) == CORE_OK);
        double e48 = max_error(c, 48, -1.0, 1.0, f_runge, NULL, 2000);

        double ratio = e48 / e64;
        CHECK(ratio > 18.0 && ratio < 32.0);
    }

    /* Clenshaw against the direct recurrence, over a domain far from the
     * origin so the mapping is doing real work. */
    {
        CHECK(cheb_fit(f_exp, NULL, -3.0, 5.0, c, 24) == CORE_OK);
        double worst = 0.0;
        for (int i = 0; i <= 400; i++) {
            double x = -3.0 + 8.0 * ((double)i / 400.0);
            double d = fabs(cheb_eval(c, 24, -3.0, 5.0, x)
                            - eval_direct(c, 24, -3.0, 5.0, x));
            if (d > worst) {
                worst = d;
            }
        }
        CHECK(worst < 1e-12);
    }

    /* Argument validation. */
    {
        CHECK(cheb_fit(NULL, NULL, 0.0, 1.0, c, 8) == CORE_ERR_INVALID_ARG);
        CHECK(cheb_fit(f_exp, NULL, 0.0, 1.0, NULL, 8) == CORE_ERR_INVALID_ARG);
        CHECK(cheb_fit(f_exp, NULL, 0.0, 1.0, c, 0) == CORE_ERR_INVALID_ARG);
        CHECK(cheb_fit(f_exp, NULL, 1.0, 1.0, c, 8) == CORE_ERR_INVALID_ARG);
        CHECK(cheb_fit(f_exp, NULL, 5.0, 1.0, c, 8) == CORE_ERR_INVALID_ARG);
        CHECK(cheb_fit(f_exp, NULL, 0.0, 1.0, c, CHEB_FIT_MAX_N + 1)
              == CORE_ERR_INVALID_ARG);
    }

    return TEST_RESULT();
}
