/* Determinism scenario: bare floating point arithmetic.
 *
 * Runs before any physics exists, and stays useful afterwards as the canary
 * that separates "the compiler or the flags changed" from "the physics
 * changed". If this scenario drifts, nothing else needs investigating first.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"

#include <math.h>
#include <stdio.h>

/* Blocks constant folding. Without it the compiler evaluates these canaries at
 * compile time and the flags under test never apply at all — measured: with
 * plain constants, -ffp-contract=off and -ffp-contract=fast produce byte
 * identical output. */
static double opaque(double x)
{
    volatile double v = x;
    return v;
}

/* Contraction canary. Evaluated as written, a*b rounds first and the sum is
 * exactly 0. Contracted into a single FMA, the product keeps its extra bits
 * and the result is 2^-53. One operation, two different answers, decided
 * entirely by -ffp-contract. */
static void canary_fma(CoreHash *h)
{
    double a = opaque(1.0 + 2.220446049250313e-16);  /* 1 + 2^-52 */
    double b = opaque(1.0 - 1.1102230246251565e-16); /* 1 - 2^-53 */
    double c = opaque(-1.0);

    core_hash_f64(h, a * b + c);
}

/* Summation order canary. The terms span many orders of magnitude, so the
 * result depends on the order they are added in. Nothing may reorder this:
 * not the compiler (which needs -ffast-math to be allowed to, and never gets
 * it), not a future parallel reduction (PROJECT.md section 4 forbids rayon in
 * physics for exactly this reason). */
static void canary_summation_order(CoreHash *h)
{
    double sum = opaque(1e16);
    for (int i = 0; i < 1000; i++) {
        sum += opaque(1.0);
    }
    core_hash_f64(h, sum);
}

/* A long chain of dependent multiplications and subtractions, of the shape the
 * integrator will actually run.
 *
 * Note on what this does and does not prove. The logistic map is chaotic, but
 * that does NOT mean it amplifies any small difference: a one ULP change to
 * the seed is absorbed completely by rounding on the very first step, because
 * it falls below half an ULP of the result. Measured, not assumed. What the
 * chain does give is length and dependency — and since every iterate is
 * hashed, a difference at any single step is recorded even if the two
 * trajectories later collapse back onto the same values. */
static void canary_iteration(CoreHash *h)
{
    double x = opaque(0.4);
    for (int i = 0; i < 10000; i++) {
        x = 3.9 * x * (1.0 - x);
        core_hash_f64(h, x);
    }
}

/* sqrt is the one libm name the deterministic zone may use: IEEE-754 requires
 * it to be correctly rounded, so it is identical everywhere. Covered here
 * precisely because it is the exception to the rule. */
static void canary_sqrt(CoreHash *h)
{
    double s = opaque(0.0);
    for (int i = 1; i <= 10000; i++) {
        s += sqrt((double)i);
    }
    core_hash_f64(h, s);
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    canary_fma(&h);
    canary_summation_order(&h);
    canary_iteration(&h);
    canary_sqrt(&h);

    printf("sc_arith %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
