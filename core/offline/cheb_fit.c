#include "cheb_fit.h"

#include <math.h>

/* Not M_PI: that is POSIX, and the core builds with -std=c11 where it is not
 * guaranteed to exist. */
static const double CHEB_PI = 3.14159265358979323846;

CoreResult cheb_fit(ChebFunc f, void *ctx, double a, double b,
                    double *c_out, size_t n)
{
    if (f == NULL || c_out == NULL || n == 0 || n > CHEB_FIT_MAX_N) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(b > a)) {
        return CORE_ERR_INVALID_ARG;
    }

    double fx[CHEB_FIT_MAX_N];

    /* Sample f at the Chebyshev-Gauss nodes cos(pi*(k+1/2)/n), mapped from
     * [-1, 1] onto [a, b]. Clustering the nodes towards the ends is what
     * keeps the error uniform instead of blowing up at the edges. */
    for (size_t k = 0; k < n; k++) {
        double node = cos(CHEB_PI * ((double)k + 0.5) / (double)n);
        double x = 0.5 * (node * (b - a) + (a + b));
        fx[k] = f(x, ctx);
    }

    for (size_t j = 0; j < n; j++) {
        double sum = 0.0;
        for (size_t k = 0; k < n; k++) {
            sum += fx[k] * cos(CHEB_PI * (double)j * ((double)k + 0.5) / (double)n);
        }
        c_out[j] = (2.0 / (double)n) * sum;
    }

    /* The series convention is c0/2 + sum(cj*Tj) for j >= 1. Halving c0 here
     * rather than in the evaluator keeps the runtime loop free of the special
     * case, which is where the cost would actually be paid. */
    c_out[0] *= 0.5;

    return CORE_OK;
}
