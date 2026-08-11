/* Guards the floating point assumptions the whole core rests on.
 *
 * These overlap with the sc_arith determinism scenario on purpose. The
 * scenario tells you *that* the result changed; these tests tell you *what*
 * broke, which is the difference between a five minute fix and an afternoon
 * of bisecting hashes. */

#include "test.h"

#include <math.h>

/* Blocks constant folding — see the same helper in sc_arith.c. Without it the
 * compiler answers these questions at compile time and the flags under test
 * are never exercised. */
static double opaque(double x)
{
    volatile double v = x;
    return v;
}

int main(void)
{
    /* -ffp-contract=off is in effect. If this fails, the compiler is fusing
     * multiply-add, results differ between machines with and without FMA
     * hardware, and determinism is gone. Check core/cflags.txt first. */
    {
        double a = opaque(1.0 + 2.220446049250313e-16);  /* 1 + 2^-52 */
        double b = opaque(1.0 - 1.1102230246251565e-16); /* 1 - 2^-53 */
        double c = opaque(-1.0);
        CHECK_BITS_EQ(a * b + c, 0.0);
    }

    /* double is IEEE-754 binary64: the gap above 1.0 is 2^-52 exactly. */
    {
        double one = opaque(1.0);
        CHECK_BITS_EQ((one + 2.220446049250313e-16) - one, 2.220446049250313e-16);
        CHECK_BITS_EQ((one + 1.1102230246251565e-16) - one, 0.0);
    }

    /* sqrt is correctly rounded, so perfect squares come back exact. This is
     * why sqrt is the one libm name allowed in the deterministic zone. */
    {
        CHECK_BITS_EQ(sqrt(opaque(4.0)), 2.0);
        CHECK_BITS_EQ(sqrt(opaque(1e100)), 1e50);
    }

    /* Addition is not associative in floating point. Stated as a test rather
     * than a comment, because every "just parallelise the force loop" idea
     * has to answer to it. */
    {
        double big = opaque(1e16), one = opaque(1.0);

        /* Same three values, two groupings, two different answers: 1.0 is
         * below half an ULP at 1e16, so adding it first loses it entirely. */
        CHECK_BITS_EQ((big + one) - big, 0.0);
        CHECK_BITS_EQ(one + (big - big), 1.0);
    }

    return TEST_RESULT();
}
