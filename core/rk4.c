#include "integrator.h"

CoreResult rk4_step(AccelFunc f, void *ctx, const State *in, double h,
                    State *out)
{
    if (f == NULL || in == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    double t = in->t;
    Vec3d r = in->r;
    Vec3d v = in->v;

    double h_half = 0.5 * h;

    /* Classic RK4 on the first-order system y = (r, v), y' = (v, a).
     * The derivative of position at each stage is that stage's velocity, so
     * k_r and k_v are carried separately rather than packed into one vector -
     * more lines, but every stage reads exactly as it is written down. */

    Vec3d k1_r = v;
    Vec3d k1_v;
    f(t, r, v, ctx, &k1_v);

    Vec3d k2_r = vec3_add_scaled(v, k1_v, h_half);
    Vec3d k2_v;
    f(t + h_half,
      vec3_add_scaled(r, k1_r, h_half),
      k2_r,
      ctx, &k2_v);

    Vec3d k3_r = vec3_add_scaled(v, k2_v, h_half);
    Vec3d k3_v;
    f(t + h_half,
      vec3_add_scaled(r, k2_r, h_half),
      k3_r,
      ctx, &k3_v);

    Vec3d k4_r = vec3_add_scaled(v, k3_v, h);
    Vec3d k4_v;
    f(t + h,
      vec3_add_scaled(r, k3_r, h),
      k4_r,
      ctx, &k4_v);

    /* (k1 + 2*k2 + 2*k3 + k4) * h/6, accumulated in stage order. */
    double h_sixth = h / 6.0;

    Vec3d sum_r = vec3_add(k1_r, vec3_scale(k2_r, 2.0));
    sum_r = vec3_add(sum_r, vec3_scale(k3_r, 2.0));
    sum_r = vec3_add(sum_r, k4_r);

    Vec3d sum_v = vec3_add(k1_v, vec3_scale(k2_v, 2.0));
    sum_v = vec3_add(sum_v, vec3_scale(k3_v, 2.0));
    sum_v = vec3_add(sum_v, k4_v);

    out->r = vec3_add_scaled(r, sum_r, h_sixth);
    out->v = vec3_add_scaled(v, sum_v, h_sixth);
    out->t = t + h;

    return CORE_OK;
}

CoreResult rk4_integrate(AccelFunc f, void *ctx, const State *in,
                         double t_end, double h, State *out)
{
    if (f == NULL || in == NULL || out == NULL || !(h > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    double span = t_end - in->t;

    State current = *in;
    if (span == 0.0) {
        *out = current;
        return CORE_OK;
    }

    double magnitude = span < 0.0 ? -span : span;
    double steps_wanted = magnitude / h;

    /* Round to at least one whole step, then divide the interval evenly.
     * Adding 0.5 and truncating rather than calling round(): libm is not
     * available in the deterministic zone. */
    long n_steps = (long)(steps_wanted + 0.5);
    if (n_steps < 1) {
        n_steps = 1;
    }

    double step = span / (double)n_steps;

    for (long i = 0; i < n_steps; i++) {
        State next;
        CoreResult r = rk4_step(f, ctx, &current, step, &next);
        if (r != CORE_OK) {
            return r;
        }
        current = next;
    }

    /* The accumulated t drifts by rounding over many steps; the requested end
     * time is what the caller asked for and what the next leg must continue
     * from. */
    current.t = t_end;

    *out = current;
    return CORE_OK;
}
