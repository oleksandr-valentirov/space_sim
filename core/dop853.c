#include "dop853_coeffs.h"
#include "integrator.h"

/* Controller constants, as in Hairer's original. */
#define SAFETY     0.9
#define MIN_SCALE  0.2   /* a rejected step may not shrink by more than this */
#define MAX_SCALE  10.0  /* nor an accepted one grow by more */

#define DEFAULT_MAX_STEPS 1000000L

/* One derivative of one block: dr/dt and dv/dt. */
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

/* One trial step of size h from (t, r, v), given the derivatives there in
 * k[0]. Writes the proposed blocks, their derivatives into k[DOP853_STAGES],
 * and the error norm of block 0. Does not decide whether to accept. */
static void try_step(BlockAccelFunc f, void *ctx, int nb,
                     double t, const Vec3d *r, const Vec3d *v, double h,
                     double tol_m, Deriv k[][DOP853_MAX_BLOCKS],
                     Vec3d *r_out, Vec3d *v_out,
                     double *error_norm, long *n_evals)
{
    Vec3d r_stage[DOP853_MAX_BLOCKS];
    Vec3d v_stage[DOP853_MAX_BLOCKS];
    Vec3d a_stage[DOP853_MAX_BLOCKS];

    for (int i = 1; i < DOP853_STAGES; i++) {
        for (int b = 0; b < nb; b++) {
            Vec3d dr = vec3_zero();
            Vec3d dv = vec3_zero();

            /* Stage sums are accumulated in stage order, including the zero
             * coefficients. Skipping them would save a few multiplies and
             * change nothing numerically, but it is exactly the kind of
             * shortcut that makes two builds disagree if someone later
             * reorders the test. */
            for (int j = 0; j < i; j++) {
                double a = DOP853_A[i][j];
                dr = vec3_add_scaled(dr, k[j][b].dr, a);
                dv = vec3_add_scaled(dv, k[j][b].dv, a);
            }

            r_stage[b] = vec3_add_scaled(r[b], dr, h);
            v_stage[b] = vec3_add_scaled(v[b], dv, h);

            k[i][b].dr = v_stage[b];
        }

        f(t + DOP853_C[i] * h, r_stage, v_stage, nb, ctx, a_stage);
        (*n_evals)++;

        for (int b = 0; b < nb; b++) {
            k[i][b].dv = a_stage[b];
        }
    }

    for (int b = 0; b < nb; b++) {
        Vec3d sum_r = vec3_zero();
        Vec3d sum_v = vec3_zero();
        for (int j = 0; j < DOP853_STAGES; j++) {
            sum_r = vec3_add_scaled(sum_r, k[j][b].dr, DOP853_B[j]);
            sum_v = vec3_add_scaled(sum_v, k[j][b].dv, DOP853_B[j]);
        }

        r_out[b] = vec3_add_scaled(r[b], sum_r, h);
        v_out[b] = vec3_add_scaled(v[b], sum_v, h);

        /* The derivative at the end of the step. Needed for the error
         * estimate, and reused as stage zero of the next step if this one is
         * accepted (first-same-as-last), so it costs nothing. */
        k[DOP853_STAGES][b].dr = v_out[b];
    }

    f(t + h, r_out, v_out, nb, ctx, a_stage);
    (*n_evals)++;

    for (int b = 0; b < nb; b++) {
        k[DOP853_STAGES][b].dv = a_stage[b];
    }

    Vec3d e5r = vec3_zero(), e5v = vec3_zero();
    Vec3d e3r = vec3_zero(), e3v = vec3_zero();
    for (int j = 0; j <= DOP853_STAGES; j++) {
        e5r = vec3_add_scaled(e5r, k[j][0].dr, DOP853_E5[j]);
        e5v = vec3_add_scaled(e5v, k[j][0].dv, DOP853_E5[j]);
        e3r = vec3_add_scaled(e3r, k[j][0].dr, DOP853_E3[j]);
        e3v = vec3_add_scaled(e3v, k[j][0].dv, DOP853_E3[j]);
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
 * wildly wrong guess costs a run of rejections.
 *
 * Reads block 0 only, for the reason given at dop853_integrate_blocks; the
 * probe still advances every block, because f is entitled to see a consistent
 * set of them. */
static double initial_step(BlockAccelFunc f, void *ctx, int nb,
                           double t0, const Vec3d *r, const Vec3d *v,
                           double tol_m, double direction,
                           const Deriv *k0, long *n_evals)
{
    double d0 = sqrt(err_norm_sq(r[0], v[0], tol_m, tol_m));
    double d1 = sqrt(err_norm_sq(k0[0].dr, k0[0].dv, tol_m, tol_m));

    double h0;
    if (d0 < 1e-5 || d1 < 1e-5) {
        h0 = 1e-6;
    } else {
        h0 = 0.01 * (d0 / d1);
    }

    Vec3d r1[DOP853_MAX_BLOCKS];
    Vec3d v1[DOP853_MAX_BLOCKS];
    Vec3d a1[DOP853_MAX_BLOCKS];

    for (int b = 0; b < nb; b++) {
        r1[b] = vec3_add_scaled(r[b], k0[b].dr, h0 * direction);
        v1[b] = vec3_add_scaled(v[b], k0[b].dv, h0 * direction);
    }

    f(t0 + h0 * direction, r1, v1, nb, ctx, a1);
    (*n_evals)++;

    Vec3d ddr = vec3_sub(v1[0], k0[0].dr);
    Vec3d ddv = vec3_sub(a1[0], k0[0].dv);
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

/* The loop itself. Every public entry point below is this function with the
 * observer left out, so there is exactly one step controller in the core -
 * which is the invariant CLAUDE.md states as "one integrator, one tolerance",
 * held by construction rather than by discipline.
 *
 * out_t, when asked for, is the time actually reached: t_end for a completed
 * run, and where it stopped for one the observer ended early. */
static CoreResult integrate_blocks(BlockAccelFunc f, void *ctx, int n_blocks,
                                   double t0, double t_end,
                                   Vec3d *r, Vec3d *v,
                                   const Dop853Config *cfg, Dop853State *io,
                                   StepObserver obs, void *obs_ctx,
                                   double *out_t)
{
    if (f == NULL || r == NULL || v == NULL || cfg == NULL || io == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (n_blocks < 1 || n_blocks > DOP853_MAX_BLOCKS) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    if (t_end == t0) {
        if (out_t != NULL) {
            *out_t = t_end;
        }
        return CORE_OK;
    }

    double direction = t_end > t0 ? 1.0 : -1.0;
    long max_steps = cfg->max_steps > 0 ? cfg->max_steps : DEFAULT_MAX_STEPS;

    Deriv k[DOP853_STAGES + 1][DOP853_MAX_BLOCKS];
    Vec3d a0[DOP853_MAX_BLOCKS];

    for (int b = 0; b < n_blocks; b++) {
        k[0][b].dr = v[b];
    }
    f(t0, r, v, n_blocks, ctx, a0);
    io->n_evals++;
    for (int b = 0; b < n_blocks; b++) {
        k[0][b].dv = a0[b];
    }

    /* Pick up the step from the previous leg, or from the config, or guess. */
    double h;
    if (io->h > 0.0) {
        h = io->h;
    } else if (cfg->h_init > 0.0) {
        h = cfg->h_init;
    } else {
        h = initial_step(f, ctx, n_blocks, t0, r, v, cfg->tol_m, direction,
                         k[0], &io->n_evals);
    }

    /* A step larger than the whole span is never useful, and without a
     * ceiling the controller compounds one without bound.
     *
     * That is not hypothetical. When a caller integrates in legs shorter than
     * the natural step - which the ephemeris cooker does constantly, stopping
     * at every fit node - each step is clamped to the remaining time, comes
     * out far more accurate than requested, and the controller multiplies h
     * by the maximum growth factor. Measured: h grew 2.3x per leg and reached
     * 2.9e14 s after twenty-five legs of 0.8 days. Once it overflows to
     * infinity a rejected step can no longer shrink it, since inf * 0.2 is
     * still inf, and the integrator rejects forever. */
    double h_ceiling = cfg->h_max > 0.0 ? cfg->h_max : dabs(t_end - t0);
    if (h > h_ceiling) {
        h = h_ceiling;
    }

    double t = t0;
    long steps = 0;
    int stopped = 0;

    Vec3d r_try[DOP853_MAX_BLOCKS];
    Vec3d v_try[DOP853_MAX_BLOCKS];

    while ((t_end - t) * direction > 0.0) {
        if (++steps > max_steps) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        if (cfg->h_min > 0.0 && h < cfg->h_min) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        /* Clamp the last step onto t_end exactly, without letting the clamp
         * contaminate the step the controller carries forward. */
        double remaining = (t_end - t) * direction;
        double h_used = h < remaining ? h : remaining;

        double error_norm;
        try_step(f, ctx, n_blocks, t, r, v, h_used * direction, cfg->tol_m, k,
                 r_try, v_try, &error_norm, &io->n_evals);

        double factor;
        if (error_norm == 0.0) {
            factor = MAX_SCALE;
        } else {
            factor = SAFETY / eighth_root(error_norm);
        }

        if (error_norm < 1.0) {
            for (int b = 0; b < n_blocks; b++) {
                r[b] = r_try[b];
                v[b] = v_try[b];
            }
            t = t + h_used * direction;
            io->n_accepted++;

            /* First-same-as-last: the derivative computed at the end of the
             * accepted step is stage zero of the next one. */
            for (int b = 0; b < n_blocks; b++) {
                k[0][b] = k[DOP853_STAGES][b];
            }

            if (factor > MAX_SCALE) {
                factor = MAX_SCALE;
            }
            h *= factor;

            /* The observer sees the step the controller chose, after the
             * controller has already decided what the next one will be. That
             * order is the whole point: whatever it does with the state, io->h
             * is left holding a step that continues this trajectory, not one
             * shortened to land on a time somebody asked for. */
            if (obs != NULL) {
                State s;
                s.r = r[0];
                s.v = v[0];
                s.t = t;
                if (obs(&s, obs_ctx)) {
                    stopped = 1;
                }
            }
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

        if (h > h_ceiling) {
            h = h_ceiling;
        }

        if (stopped) {
            break;
        }
    }

    io->h = h;
    if (out_t != NULL) {
        *out_t = stopped ? t : t_end;
    }
    return CORE_OK;
}

CoreResult dop853_integrate_blocks(BlockAccelFunc f, void *ctx, int n_blocks,
                                   double t0, double t_end,
                                   Vec3d *r, Vec3d *v,
                                   const Dop853Config *cfg, Dop853State *io)
{
    return integrate_blocks(f, ctx, n_blocks, t0, t_end, r, v, cfg, io,
                            NULL, NULL, NULL);
}

/* ---- The one-block case ------------------------------------------------- */

typedef struct {
    AccelFunc f;
    void     *ctx;
} SingleCtx;

static void single_block(double t, const Vec3d *r, const Vec3d *v,
                         int n_blocks, void *ctx, Vec3d *a_out)
{
    (void)n_blocks;
    const SingleCtx *s = (const SingleCtx *)ctx;
    s->f(t, r[0], v[0], s->ctx, &a_out[0]);
}

static CoreResult integrate_single(AccelFunc f, void *ctx, const State *in,
                                   double t_end, const Dop853Config *cfg,
                                   Dop853State *io, StepObserver obs,
                                   void *obs_ctx, State *out)
{
    if (f == NULL || in == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    SingleCtx single = { f, ctx };
    Vec3d r = in->r;
    Vec3d v = in->v;
    double t_reached = t_end;

    CoreResult res = integrate_blocks(single_block, &single, 1,
                                      in->t, t_end, &r, &v, cfg, io,
                                      obs, obs_ctx, &t_reached);
    if (res != CORE_OK) {
        return res;
    }

    out->r = r;
    out->v = v;
    out->t = t_reached;
    return CORE_OK;
}

CoreResult dop853_integrate(AccelFunc f, void *ctx, const State *in,
                            double t_end, const Dop853Config *cfg,
                            Dop853State *io, State *out)
{
    return integrate_single(f, ctx, in, t_end, cfg, io, NULL, NULL, out);
}

CoreResult dop853_integrate_obs(AccelFunc f, void *ctx, const State *in,
                                double t_end, const Dop853Config *cfg,
                                Dop853State *io, StepObserver obs,
                                void *obs_ctx, State *out)
{
    return integrate_single(f, ctx, in, t_end, cfg, io, obs, obs_ctx, out);
}
