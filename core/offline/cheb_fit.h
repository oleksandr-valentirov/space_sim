/* Chebyshev fitting - OFFLINE ONLY (ROADMAP B2).
 *
 * Everything under core/offline is cooker-side: it runs when assets are
 * built, never inside the simulation. It compiles into libcore_offline.a,
 * which is linked against libm; libcore.a is not. So the determinism boundary
 * from PROJECT.md section 4 is a property of the build graph rather than a
 * rule someone has to remember.
 *
 * Concretely: fitting needs cos() for the Chebyshev nodes, and cos() is not
 * guaranteed bit-identical between platforms. That is fine here, because the
 * coefficients this produces are baked into a versioned binary asset on one
 * machine and shipped. It would not be fine one directory up. */

#ifndef CORE_CHEB_FIT_H
#define CORE_CHEB_FIT_H

#include "core.h"

#include <stddef.h>

#define CHEB_FIT_MAX_N 128

typedef double (*ChebFunc)(double x, void *ctx);

/* Fits f on [a, b] to n Chebyshev coefficients by interpolation at the n
 * Chebyshev-Gauss nodes. Writes n coefficients to c_out, already scaled so
 * that cheb_eval can sum them directly.
 *
 * Interpolation at those nodes, not least squares: it is near-optimal for
 * smooth functions, needs only n evaluations of f, and has no normal equation
 * to be ill-conditioned. For an ephemeris, where f is as smooth as functions
 * get, there is nothing to gain from anything cleverer. */
CoreResult cheb_fit(ChebFunc f, void *ctx, double a, double b,
                    double *c_out, size_t n);

#endif /* CORE_CHEB_FIT_H */
