#include "correct.h"

#define DEFAULT_MAX_ITERATIONS 20

/* How finely the scan looks for the sign change in y, and how far past the
 * guess it keeps looking. Coarse on purpose: the scan only has to produce a
 * bracket, and the refinement afterwards is what carries the accuracy. */
#define SCAN_SAMPLES 40
#define SCAN_REACH   3.0

/* Iterations of the bracketed root search. Newton converges in a handful; the
 * limit is there for the bisection fallback, which needs about fifty to
 * exhaust a double. */
#define ROOT_ITERATIONS 100

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

static Dop853Config integrator_config(double tol)
{
    Dop853Config cfg;
    cfg.tol_m = tol;
    cfg.h_init = 0.0;
    cfg.h_min = 0.0;
    cfg.h_max = 0.0;
    cfg.max_steps = 10000000;
    return cfg;
}

/* Propagate from s0 to t, always from a zeroed step history.
 *
 * That is what makes the corrector reproducible: the result depends on (s0, t)
 * and nothing else, so the same seed gives the same answer whatever order the
 * caller asked for things in. An adaptive step carried over from a previous
 * call would make it depend on the sequence of calls as well.
 *
 * It also means the trajectory here is bit-identical to the one stm_integrate
 * produces for the same arguments: block 0's arithmetic does not depend on how
 * many blocks travel with it, and the step controller reads block 0 only. The
 * crossing found with this cheap call is therefore the crossing of the very
 * trajectory whose sensitivities are measured next. */
static CoreResult propagate(const Cr3bpCtx *ctx, const State *s0, double t,
                            double tol, State *out)
{
    Dop853Config cfg = integrator_config(tol);
    Dop853State st = { 0.0, 0, 0, 0 };

    return dop853_integrate(accel_cr3bp, (void *)ctx, s0, t, &cfg, &st, out);
}

/* First crossing of y = 0 after t = 0.
 *
 * The state starts on the plane, so t = 0 is itself a root and has to be
 * stepped over. A guard of a twentieth of the guessed half period does that:
 * comfortably clear of the start, and far short of the crossing being looked
 * for, for any orbit whose period guess is within a factor of a few. */
static CoreResult find_crossing(const Cr3bpCtx *ctx, const State *s0,
                                double t_guess, double tol,
                                double *t_cross, State *out)
{
    double t_lo = 0.05 * t_guess;
    State s_lo;
    if (propagate(ctx, s0, t_lo, tol, &s_lo) != CORE_OK) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    int sign_lo = s_lo.r.y < 0.0;
    double dt = t_guess * SCAN_REACH / (double)SCAN_SAMPLES;

    double t_hi = 0.0;
    int bracketed = 0;

    /* The scan continues from where it is rather than restarting, because it
     * is only looking for a sign change. The bracket it produces is good for
     * the restarted trajectory too: the two differ by a few times the
     * tolerance, and the bracket ends are a whole scan step apart. */
    Dop853Config cfg = integrator_config(tol);
    Dop853State st = { 0.0, 0, 0, 0 };
    State walker = s_lo;

    for (int i = 1; i <= SCAN_SAMPLES; i++) {
        double t = t_lo + dt * (double)i;
        State next;
        if (dop853_integrate(accel_cr3bp, (void *)ctx, &walker, t, &cfg, &st,
                             &next) != CORE_OK) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        walker = next;

        if ((next.r.y < 0.0) != sign_lo) {
            t_hi = t;
            bracketed = 1;
            break;
        }
        t_lo = t;
    }

    if (!bracketed) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    /* Newton on y(t), whose derivative is vy and comes for free, safeguarded
     * by the bracket: a Newton step that leaves the bracket is replaced by a
     * bisection. Near a halo crossing vy is far from zero, so Newton does the
     * work and the safeguard never fires - but it costs nothing and the
     * near-rectilinear end of the family is not a place to assume good
     * behaviour. */
    double t = 0.5 * (t_lo + t_hi);
    State s;
    if (propagate(ctx, s0, t, tol, &s) != CORE_OK) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    for (int i = 0; i < ROOT_ITERATIONS; i++) {
        if (t_hi - t_lo < 1e-15 * t_guess) {
            break;
        }

        if ((s.r.y < 0.0) == sign_lo) {
            t_lo = t;
        } else {
            t_hi = t;
        }

        double t_next;
        if (s.v.y != 0.0) {
            t_next = t - s.r.y / s.v.y;
        } else {
            t_next = 0.5 * (t_lo + t_hi);
        }
        if (!(t_next > t_lo) || !(t_next < t_hi)) {
            t_next = 0.5 * (t_lo + t_hi);
        }

        if (t_next == t) {
            break;
        }
        t = t_next;

        if (propagate(ctx, s0, t, tol, &s) != CORE_OK) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
    }

    *t_cross = t;
    *out = s;
    return CORE_OK;
}

/* Impose the symmetry the family is defined by. */
static State on_the_plane(const State *seed)
{
    State s;
    s.r = vec3(seed->r.x, 0.0, seed->r.z);
    s.v = vec3(0.0, seed->v.y, 0.0);
    s.t = 0.0;
    return s;
}

CoreResult halo_correct(double mu, const State *seed, double period_guess,
                        const HaloCorrectConfig *cfg, HaloOrbit *out)
{
    if (seed == NULL || cfg == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(mu > 0.0) || !(mu < 1.0) || !(period_guess > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol > 0.0) || !(cfg->integrator_tol > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (cfg->hold != HALO_HOLD_Z && cfg->hold != HALO_HOLD_X) {
        return CORE_ERR_INVALID_ARG;
    }

    Cr3bpCtx ctx = { mu };
    State s = on_the_plane(seed);

    /* Which component of the initial state moves alongside vy. */
    int free_index = cfg->hold == HALO_HOLD_Z ? 0 : 2;

    int max_iterations = cfg->max_iterations > 0 ? cfg->max_iterations
                                                 : DEFAULT_MAX_ITERATIONS;
    double t_guess = 0.5 * period_guess;

    for (int iter = 1; iter <= max_iterations; iter++) {
        double t_cross;
        State end;
        if (find_crossing(&ctx, &s, t_guess, cfg->integrator_tol,
                          &t_cross, &end) != CORE_OK) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        t_guess = t_cross;

        double residual = dabs(end.v.x) + dabs(end.v.z);
        if (residual < cfg->tol) {
            out->s = s;
            out->period = 2.0 * t_cross;
            out->jacobi = cr3bp_jacobi(s.r, s.v, mu);
            out->residual = residual;
            out->iterations = iter;
            return CORE_OK;
        }

        /* Sensitivities at the crossing. Same trajectory, same crossing: see
         * the note on propagate. */
        Dop853Config icfg = integrator_config(cfg->integrator_tol);
        Dop853State st = { 0.0, 0, 0, 0 };
        double phi[STM_SIZE];
        State check;
        if (stm_integrate(accel_cr3bp_var, &ctx, &s, t_cross, &icfg, &st,
                          &check, phi) != CORE_OK) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        Vec3d a_end;
        accel_cr3bp(t_cross, end.r, end.v, &ctx, &a_end);

        if (end.v.y == 0.0) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        /* The correction for the crossing moving.
         *
         * Phi says where the state at a FIXED time goes when the start moves.
         * What is wanted is where the state at the CROSSING goes, and the
         * crossing moves too: holding y = 0 requires
         *
         *     Phi[1][k] dk + vy dt = 0   ->   dt = -Phi[1][k] dk / vy
         *
         * and the target components pick up a_end * dt from that. Written as a
         * per-column factor so the two rows share it. */
        double px = phi[1 * 6 + free_index];
        double pq = phi[1 * 6 + 4];

        double m00 = phi[3 * 6 + free_index] - (a_end.x / end.v.y) * px;
        double m01 = phi[3 * 6 + 4]          - (a_end.x / end.v.y) * pq;
        double m10 = phi[5 * 6 + free_index] - (a_end.z / end.v.y) * px;
        double m11 = phi[5 * 6 + 4]          - (a_end.z / end.v.y) * pq;

        double det = m00 * m11 - m01 * m10;
        if (det == 0.0) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        double f0 = -end.v.x;
        double f1 = -end.v.z;

        double d_free = (f0 * m11 - f1 * m01) / det;
        double d_vy   = (m00 * f1 - m10 * f0) / det;

        if (cfg->max_step > 0.0) {
            double biggest = dabs(d_free) > dabs(d_vy) ? dabs(d_free)
                                                       : dabs(d_vy);
            if (biggest > cfg->max_step) {
                double scale = cfg->max_step / biggest;
                d_free *= scale;
                d_vy *= scale;
            }
        }

        if (free_index == 0) {
            s.r.x += d_free;
        } else {
            s.r.z += d_free;
        }
        s.v.y += d_vy;
    }

    return CORE_ERR_TOLERANCE_NOT_MET;
}

CoreResult halo_family(double mu, const HaloOrbit *seed, double step,
                       const HaloCorrectConfig *cfg,
                       HaloOrbit *out, size_t cap, size_t *out_count)
{
    if (seed == NULL || cfg == NULL || out == NULL || out_count == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (cap == 0 || step == 0.0) {
        return CORE_ERR_INVALID_ARG;
    }

    *out_count = 0;

    HaloOrbit current = *seed;

    for (size_t i = 0; i < cap; i++) {
        State next = current.s;

        /* Step the held variable, and use the previous orbit unchanged for the
         * free ones. Linear extrapolation along the family would take larger
         * steps, but it needs two members to start from and it hides a failure
         * to converge behind a good guess. Constant continuation is the honest
         * version: when it stops converging, the step really is too large. */
        if (cfg->hold == HALO_HOLD_Z) {
            next.r.z += step;
        } else {
            next.r.x += step;
        }

        HaloOrbit found;
        if (halo_correct(mu, &next, current.period, cfg, &found) != CORE_OK) {
            return CORE_OK;
        }

        out[i] = found;
        *out_count = i + 1;
        current = found;
    }

    return CORE_OK;
}
