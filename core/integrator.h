/* Integrators (ROADMAP B3, B4).
 *
 * RK4 is here as a stepping stone, not as the answer. The runtime integrator
 * is DOP853 (PROJECT.md section 4). RK4 exists because it is thirty lines,
 * which means it validates the force model, the state layout and the test
 * harness before there is any question of looking for a mistake inside an
 * adaptive step controller. Once DOP853 lands, RK4 stays as the independent
 * second implementation to cross-check it against. */

#ifndef CORE_INTEGRATOR_H
#define CORE_INTEGRATOR_H

#include "accel.h"
#include "core.h"

#include <stddef.h>

/* One fixed step of size h. in and out may not alias. */
CoreResult rk4_step(AccelFunc f, void *ctx, const State *in, double h,
                    State *out);

/* Fixed-step integration from in->t to t_end.
 *
 * h is a request, not a promise: the interval is divided into a whole number
 * of equal steps close to h, so the end time is reached exactly and every
 * step is the same size. A ragged final step would quietly contaminate any
 * measurement of the method's order, which is the main thing RK4 is here to
 * demonstrate.
 *
 * The direction of h is ignored; t_end decides whether time runs forwards or
 * backwards. Backwards integration is what the reversibility test needs. */
CoreResult rk4_integrate(AccelFunc f, void *ctx, const State *in,
                         double t_end, double h, State *out);

#endif /* CORE_INTEGRATOR_H */
