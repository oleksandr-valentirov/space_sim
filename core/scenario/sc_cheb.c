/* Determinism scenario: Chebyshev evaluation.
 *
 * cheb_eval is runtime code — the ephemeris is read through it on every force
 * evaluation — so it gets its own golden hash, independent of sc_arith and
 * sc_vec3.
 *
 * Coefficients are generated arithmetically rather than fitted: cheb_fit
 * lives in libcore_offline.a, and scenarios deliberately link only against
 * libcore.a with no libm. This file failing to link would mean the
 * determinism boundary had been breached.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cheb.h"
#include "hash.h"

#include <stdio.h>

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    double c[32];
    for (size_t j = 0; j < 32; j++) {
        double sign = (j % 2 == 0) ? 1.0 : -1.0;
        c[j] = opaque(sign / ((double)j + 1.0));
    }

    /* Sweep the domain, including both endpoints where the mapping lands
     * exactly on -1 and +1, and a span far from zero so the mapping division
     * is doing real work. */
    double a = opaque(-3.0);
    double b = opaque(1.7e6);

    for (int i = 0; i <= 5000; i++) {
        double x = a + (b - a) * ((double)i / 5000.0);
        core_hash_f64(&h, cheb_eval(c, 32, a, b, x));

        /* The derivative is runtime code too: every velocity the ephemeris
         * reports comes through it. */
        core_hash_f64(&h, cheb_eval_deriv(c, 32, a, b, x));
    }

    /* Short series and the degenerate case, since they take different paths
     * through the recurrence. */
    for (size_t n = 0; n <= 4; n++) {
        core_hash_f64(&h, cheb_eval(c, n, a, b, opaque(12345.0)));
        core_hash_f64(&h, cheb_eval_deriv(c, n, a, b, opaque(12345.0)));
    }

    printf("sc_cheb %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
