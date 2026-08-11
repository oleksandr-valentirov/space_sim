#include "dop853_coeffs.h"
#include "integrator.h"

/* Controller constants, as in Hairer's original. */
#define SAFETY     0.9
#define MIN_SCALE  0.2   /* a rejected step may not shrink by more than this */
#define MAX_SCALE  10.0  /* nor an accepted one grow by more */

#define DEFAULT_MAX_STEPS 1000000L

/* One derivative of the state: dr/dt and dv/dt. */
typedef struct {
    Vec3d dr;
    Vec3d dv;
} Deriv;

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* x^(1/8), which is what an eighth-order step controller needs.
 *
 * pow() is libm and forbidden in the deterministic zone (PROJECT.md section
 * 4). Three nested square roots give exactly the exponent required, and sqrt
 * is correctly rounded by IEEE-754, so this is bit-identical on every
 * platform. The controller does not need more accuracy than that: its output
 * is a step size, and the error estimate it is derived from is itself only an
 * estimate. */
static double eighth_root(double x)
{
    return sqrt(sqrt(sqrt(x)));
}

/* Squared 6-component weighted norm of a state-shaped error. */
static double err_norm_sq(Vec3d er, Vec3d ev, double scale_r, double scale_v)
{
    double xr = er.x / scale_r;
    double yr = er.y / scale_r;
    double zr = er.z / scale_r;
    double xv = ev.x / scale_v;
    double yv = ev.y / scale_v;
    double zv = ev.z / scale_v;

    return xr * xr + yr * yr + zr * zr + xv * xv + yv * yv + zv * zv;
}

/* One trial step of size h from (t, r, v), given the derivative there in
 * k[0]. Writes the proposed state, its derivative into k[DOP853_STAGES], and
 * the error norm. Does not decide whether to accept. */
static void dop853_try_step(AccelFunc f, void *ctx,
                            double t, Vec3d r, Vec3d v, double h,
                            double tol_m, Deriv *k,
                            State *out, double *error_norm, long *n_evals)
{
    for (int i = 1; i < DOP853_STAGES; i++) {
        Vec3d dr = vec3_zero();
        Vec3d dv = vec3_zero();

        /* Stage sums are accumulated in stage order, including the zero
         * coefficients. Skipping them would save a few multiplies and change
         * nothing numerically, but it is exactly the kind of shortcut that
         * makes two builds disagree if someone later reorders the test. */
        for (int j = 0; j < i; j++) {
            double a = DOP853_A[i][j];
            dr = vec3_add_scaled(dr, k[j].dr, a);
            dv = vec3_add_scaled(dv, k[j].dv, a);
        }

        Vec3d r_stage = vec3_add_scaled(r, dr, h);
        Vec3d v_stage = vec3_add_scaled(v, dv, h);

        k[i].dr = v_stage;
        f(t + DOP853_C[i] * h, r_stage, v_stage, ctx, &k[i].dv);
        (*n_evals)++;
    }

    Vec3d sum_r = vec3_zero();
    Vec3d sum_v = vec3_zero();
    for (int j = 0; j < DOP853_STAGES; j++) {
        sum_r = vec3_add_scaled(sum_r, k[j].dr, DOP853_B[j]);
        sum_v = vec3_add_scaled(sum_v, k[j].dv, DOP853_B[j]);
    }

    out->r = vec3_add_scaled(r, sum_r, h);
    out->v = vec3_add_scaled(v, sum_v, h);
    out->t = t + h;

    /* The derivative at the end of the step. Needed for the error estimate,
     * and reused as stage zero of the next step if this one is accepted
     * (first-same-as-last), so it costs nothing. */
    k[DOP853_STAGES].dr = out->v;
    f(out->t, out->r, out->v, ctx, &k[DOP853_STAGES].dv);
    (*n_evals)++;

    Vec3d e5r = vec3_zero(), e5v = vec3_zero();
    Vec3d e3r = vec3_zero(), e3v = vec3_zero();
    for (int j = 0; j <= DOP853_STAGES; j++) {
        e5r = vec3_add_scaled(e5r, k[j].dr, DOP853_E5[j]);
        e5v = vec3_add_scaled(e5v, k[j].dv, DOP853_E5[j]);
        e3r = vec3_add_scaled(e3r, k[j].dr, DOP853_E3[j]);
        e3v = vec3_add_scaled(e3v, k[j].dv, DOP853_E3[j]);
    }

    double scale_r = tol_m;
    double scale_v = tol_m / dabs(h);

    double n5 = err_norm_sq(e5r, e5v, scale_r, scale_v);
    double n3 = err_norm_sq(e3r, e3v, scale_r, scale_v);

    /* Hairer's combined estimator: the fifth-order estimate carries the
     * decision, and the third-order one damps it where the fifth happens to
     * be near zero by accident rather than by accuracy. */
    double denom = n5 + 0.01 * n3;
    if (denom <= 0.0) {
        *error_norm = 0.0;
    } else {
        *error_norm = dabs(h) * n5 / sqrt(denom * 6.0);
    }
}

/* Hairer's starting step heuristic: guess from the ratio of the state to its
 * derivative, take one Euler probe, and refine from the curvature it reveals.
 * Getting this roughly right matters only for the first few steps, but a
 * wildly wrong guess costs a run of rejections. */
static double initial_step(AccelFunc f, void *ctx, const State *s,
                           double tol_m, double direction,
                           const Deriv *k0, long *n_evals)
{
    double d0 = sqrt(err_norm_sq(s->r, s->v, tol_m, tol_m));
    double d1 = sqrt(err_norm_sq(k0->dr, k0->dv, tol_m, tol_m));

    double h0;
    if (d0 < 1e-5 || d1 < 1e-5) {
        h0 = 1e-6;
    } else {
        h0 = 0.01 * (d0 / d1);
    }

    Vec3d r1 = vec3_add_scaled(s->r, k0->dr, h0 * direction);
    Vec3d v1 = vec3_add_scaled(s->v, k0->dv, h0 * direction);

    Vec3d a1;
    f(s->t + h0 * direction, r1, v1, ctx, &a1);
    (*n_evals)++;

    Vec3d ddr = vec3_sub(v1, k0->dr);
    Vec3d ddv = vec3_sub(a1, k0->dv);
    double d2 = sqrt(err_norm_sq(ddr, ddv, tol_m, tol_m)) / h0;

    double d_max = d1 > d2 ? d1 : d2;

    double h1;
    if (d_max <= 1e-15) {
        double alt = h0 * 1e-3;
        h1 = alt > 1e-6 ? alt : 1e-6;
    } else {
        h1 = eighth_root(0.01 / d_max);
    }

    double capped = 100.0 * h0;
    return h1 < capped ? h1 : capped;
}

CoreResult dop853_integrate(AccelFunc f, void *ctx, const State *in,
                            double t_end, const Dop853Config *cfg,
                            Dop853State *io, State *out)
{
    if (f == NULL || in == NULL || cfg == NULL || io == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    State current = *in;

    if (t_end == current.t) {
        *out = current;
        return CORE_OK;
    }

    double direction = t_end > current.t ? 1.0 : -1.0;
    long max_steps = cfg->max_steps > 0 ? cfg->max_steps : DEFAULT_MAX_STEPS;

    Deriv k[DOP853_STAGES + 1];
    k[0].dr = current.v;
    f(current.t, current.r, current.v, ctx, &k[0].dv);
    io->n_evals++;

    /* Pick up the step from the previous leg, or from the config, or guess. */
    double h;
    if (io->h > 0.0) {
        h = io->h;
    } else if (cfg->h_init > 0.0) {
        h = cfg->h_init;
    } else {
        h = initial_step(f, ctx, &current, cfg->tol_m, direction, &k[0],
                         &io->n_evals);
    }

    if (cfg->h_max > 0.0 && h > cfg->h_max) {
        h = cfg->h_max;
    }

    long steps = 0;

    while ((t_end - current.t) * direction > 0.0) {
        if (++steps > max_steps) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        if (cfg->h_min > 0.0 && h < cfg->h_min) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        /* Clamp the last step onto t_end exactly, without letting the clamp
         * contaminate the step the controller carries forward. */
        double remaining = (t_end - current.t) * direction;
        double h_used = h < remaining ? h : remaining;

        State candidate;
        double error_norm;
        dop853_try_step(f, ctx, current.t, current.r, current.v,
                        h_used * direction, cfg->tol_m, k,
                        &candidate, &error_norm, &io->n_evals);

        double factor;
        if (error_norm == 0.0) {
            factor = MAX_SCALE;
        } else {
            factor = SAFETY / eighth_root(error_norm);
        }

        if (error_norm < 1.0) {
            current = candidate;
            io->n_accepted++;

            /* First-same-as-last: the derivative computed at the end of the
             * accepted step is stage zero of the next one. */
            k[0] = k[DOP853_STAGES];

            if (factor > MAX_SCALE) {
                factor = MAX_SCALE;
            }
            h *= factor;
        } else {
            io->n_rejected++;

            if (factor > 1.0) {
                factor = 1.0;
            }
            if (factor < MIN_SCALE) {
                factor = MIN_SCALE;
            }
            h *= factor;
        }

        if (cfg->h_max > 0.0 && h > cfg->h_max) {
            h = cfg->h_max;
        }
    }

    current.t = t_end;
    io->h = h;
    *out = current;
    return CORE_OK;
}
