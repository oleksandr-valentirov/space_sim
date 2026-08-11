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
