/* Chebyshev series evaluation (ROADMAP B2).
 *
 * This is how the ephemeris is stored and read: positions are fitted offline
 * to Chebyshev polynomials and the runtime only evaluates them (PROJECT.md
 * section 4). The same approach JPL SPICE uses, for the same reason - the
 * evaluation is a short loop of multiplies and adds with no table lookups and
 * no branches.
 *
 * Evaluation lives here, in the deterministic zone: Clenshaw recurrence, only
 * + - * and one division for the domain mapping. Fitting lives in
 * core/offline/cheb_fit.h because it needs cos(), and libm is not allowed on
 * this side of the line. That split is enforced by the build, not by
 * convention: this file compiles into libcore.a, which is never linked
 * against libm. */

#ifndef CORE_CHEB_H
#define CORE_CHEB_H

#include <stddef.h>

/* Evaluates sum(c[j] * T_j(s)) for j in [0, n), where s is x mapped from
 * [a, b] onto [-1, 1]. c[0] is the full coefficient of T_0, so the halving
 * convention is already baked into the stored coefficients by cheb_fit.
 *
 * x outside [a, b] is extrapolation: the series is still evaluated, but the
 * result is meaningless. Callers pick the right interval; checking here would
 * cost a branch in the innermost loop of the whole simulation. */
double cheb_eval(const double *c, size_t n, double a, double b, double x);

#endif /* CORE_CHEB_H */
