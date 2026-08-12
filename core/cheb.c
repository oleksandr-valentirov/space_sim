#include "cheb.h"

double cheb_eval(const double *c, size_t n, double a, double b, double x)
{
    if (n == 0) {
        return 0.0;
    }

    /* Map [a, b] onto [-1, 1]. The one division in the whole routine. */
    double s = (2.0 * x - (a + b)) / (b - a);
    double s2 = 2.0 * s;

    /* Clenshaw recurrence, downward. Operation order is fixed and must stay
     * that way: this runs inside the integrator, where a reassociated sum is
     * a different trajectory. */
    double d = 0.0;
    double dd = 0.0;

    for (size_t j = n - 1; j >= 1; j--) {
        double previous = d;
        d = s2 * d - dd + c[j];
        dd = previous;
    }

    return s * d - dd + c[0];
}

double cheb_eval_deriv(const double *c, size_t n, double a, double b, double x)
{
    if (n < 2) {
        return 0.0;
    }

    double s = (2.0 * x - (a + b)) / (b - a);

    /* dT_j/ds = j * U_{j-1}, so the derivative is a sum over Chebyshev
     * polynomials of the second kind, built forwards by their own recurrence:
     * U_0 = 1, U_1 = 2s, U_k = 2s*U_{k-1} - U_{k-2}.
     *
     * Forward rather than a Clenshaw variant: the recurrence is the identity
     * being used, written out, and this runs often enough that it should be
     * readable when someone comes looking for a discrepancy. */
    double u_previous = 0.0;   /* U_{-1} */
    double u = 1.0;            /* U_0 */

    double sum = c[1] * u;

    for (size_t j = 2; j < n; j++) {
        double u_next = 2.0 * s * u - u_previous;
        sum += c[j] * (double)j * u_next;
        u_previous = u;
        u = u_next;
    }

    /* ds/dx for the mapping onto [-1, 1]. */
    return sum * (2.0 / (b - a));
}
