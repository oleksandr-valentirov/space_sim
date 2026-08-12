#include "shooting.h"

#include "stm.h"

#include <string.h>

#define DEFAULT_MAX_ITERATIONS 20

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* ---- 6x6 linear algebra ------------------------------------------------- */

/* Solve a x = b for x, where a is 6x6 and b is 6 by `cols`, all row-major.
 * a and b are destroyed. Returns 0 if a is singular.
 *
 * Gaussian elimination with partial pivoting, and the pivoting is not
 * decorative: the blocks here are Phi Phi^T + I, which is positive definite in
 * exact arithmetic but is assembled from a transition matrix whose entries
 * differ by orders of magnitude even after scaling. */
static int solve6(double a[36], double *b, int cols, double *x)
{
    int row[6];
    for (int i = 0; i < 6; i++) {
        row[i] = i;
    }

    for (int col = 0; col < 6; col++) {
        int best = col;
        double best_size = dabs(a[row[col] * 6 + col]);
        for (int k = col + 1; k < 6; k++) {
            double size = dabs(a[row[k] * 6 + col]);
            if (size > best_size) {
                best_size = size;
                best = k;
            }
        }

        if (best_size == 0.0) {
            return 0;
        }

        int swap = row[col];
        row[col] = row[best];
        row[best] = swap;

        double pivot = a[row[col] * 6 + col];

        for (int k = col + 1; k < 6; k++) {
            double factor = a[row[k] * 6 + col] / pivot;
            if (factor == 0.0) {
                continue;
            }
            for (int j = col; j < 6; j++) {
                a[row[k] * 6 + j] -= factor * a[row[col] * 6 + j];
            }
            for (int j = 0; j < cols; j++) {
                b[row[k] * cols + j] -= factor * b[row[col] * cols + j];
            }
        }
    }

    for (int col = 5; col >= 0; col--) {
        for (int j = 0; j < cols; j++) {
            double sum = b[row[col] * cols + j];
            for (int k = col + 1; k < 6; k++) {
                sum -= a[row[col] * 6 + k] * x[k * cols + j];
            }
            x[col * cols + j] = sum / a[row[col] * 6 + col];
        }
    }

    return 1;
}

/* c = a b^T, all 6x6 row-major. */
static void mul_transpose(const double a[36], const double b[36], double c[36])
{
    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            double sum = 0.0;
            for (int k = 0; k < 6; k++) {
                sum += a[i * 6 + k] * b[j * 6 + k];
            }
            c[i * 6 + j] = sum;
        }
    }
}

/* y = a x, 6x6 times 6. */
static void apply(const double a[36], const double x[6], double y[6])
{
    for (int i = 0; i < 6; i++) {
        double sum = 0.0;
        for (int k = 0; k < 6; k++) {
            sum += a[i * 6 + k] * x[k];
        }
        y[i] = sum;
    }
}

/* y = a^T x. */
static void apply_transpose(const double a[36], const double x[6], double y[6])
{
    for (int i = 0; i < 6; i++) {
        double sum = 0.0;
        for (int k = 0; k < 6; k++) {
            sum += a[k * 6 + i] * x[k];
        }
        y[i] = sum;
    }
}

/* ---- scaling ------------------------------------------------------------ */

/* State to scaled vector and back. Positions in units of length_scale,
 * velocities in units of speed_scale. */
static void pack(const State *s, double length, double speed, double v[6])
{
    v[0] = s->r.x / length;
    v[1] = s->r.y / length;
    v[2] = s->r.z / length;
    v[3] = s->v.x / speed;
    v[4] = s->v.y / speed;
    v[5] = s->v.z / speed;
}

/* Rescale a transition matrix into the same units: Phi~ = S Phi S^-1, which
 * for a diagonal S is a multiplication of each entry by s_row / s_col. */
static void rescale(double phi[36], double length, double speed)
{
    double scale[6] = { length, length, length, speed, speed, speed };

    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            phi[i * 6 + j] *= scale[j] / scale[i];
        }
    }
}

/* ---- the correction ----------------------------------------------------- */

CoreResult shoot_multiple(BlockAccelFunc f, void *ctx,
                          State *states, const double *times, size_t n,
                          const ShootingConfig *cfg,
                          double *workspace, size_t workspace_len,
                          ShootingReport *report)
{
    if (f == NULL || states == NULL || times == NULL || cfg == NULL ||
        workspace == NULL || report == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (n < 2) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol_m > 0.0) || !(cfg->continuity_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->length_scale > 0.0) || !(cfg->speed_scale > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (workspace_len < SHOOTING_WORKSPACE(n)) {
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    memset(report, 0, sizeof *report);

    size_t legs = n - 1;

    /* Workspace layout: one transition matrix and one elimination block per
     * leg, plus the right-hand side that becomes the multiplier. */
    double *phi = workspace;                    /* legs * 36 */
    double *chat = phi + legs * 36;             /* legs * 36 */
    double *yhat = chat + legs * 36;            /* legs * 6  */
    double *guess = yhat + legs * 6;            /* n * 6     */

    /* The guess is kept because the correction is measured against it, not
     * against the previous iterate. See the header. */
    for (size_t i = 0; i < n; i++) {
        pack(&states[i], cfg->length_scale, cfg->speed_scale, &guess[i * 6]);
    }

    double length = cfg->length_scale;
    double speed = cfg->speed_scale;

    int max_iterations = cfg->max_iterations > 0 ? cfg->max_iterations
                                                 : DEFAULT_MAX_ITERATIONS;

    Dop853Config leg_cfg;
    memset(&leg_cfg, 0, sizeof leg_cfg);
    leg_cfg.tol_m = cfg->tol_m;
    leg_cfg.max_steps = 20000000;

    /* Kept so the report can say how far the answer moved from the guess. */
    double worst_step = 0.0;

    for (int iteration = 1; iteration <= max_iterations; iteration++) {
        double worst_position = 0.0;
        double worst_velocity = 0.0;

        /* Defects, scaled, stored where the multipliers will go. */
        for (size_t i = 0; i < legs; i++) {
            State from = states[i];
            from.t = times[i];

            Dop853State st;
            memset(&st, 0, sizeof st);

            State arrival;
            CoreResult r = stm_integrate(f, ctx, &from, times[i + 1], &leg_cfg,
                                         &st, &arrival, &phi[i * 36]);
            if (r != CORE_OK) {
                return r;
            }

            double gap_r = vec3_distance(arrival.r, states[i + 1].r);
            double gap_v = vec3_norm(vec3_sub(arrival.v, states[i + 1].v));
            if (gap_r > worst_position) {
                worst_position = gap_r;
            }
            if (gap_v > worst_velocity) {
                worst_velocity = gap_v;
            }

            double a[6], b[6];
            pack(&arrival, length, speed, a);
            pack(&states[i + 1], length, speed, b);

            rescale(&phi[i * 36], length, speed);

            /* Right-hand side J e - F, where e is the displacement of the
             * current iterate from the guess and F is the defect. The first
             * iteration has e = 0 and this is the plain Newton step; later
             * ones carry the pull back towards the guess. */
            double here[6], next[6], drift_here[6], drift_next[6];
            pack(&states[i], length, speed, here);
            for (int k = 0; k < 6; k++) {
                drift_here[k] = here[k] - guess[i * 6 + k];
                drift_next[k] = b[k] - guess[(i + 1) * 6 + k];
            }

            apply(&phi[i * 36], drift_here, next);

            for (int k = 0; k < 6; k++) {
                yhat[i * 6 + k] = (next[k] - drift_next[k]) - (a[k] - b[k]);
            }
        }

        report->iterations = iteration;
        report->worst_position_gap = worst_position;
        report->worst_velocity_gap = worst_velocity;
        report->worst_step_m = worst_step;

        if (worst_position < cfg->continuity_m) {
            return CORE_OK;
        }

        /* Block elimination on J J^T, whose diagonal blocks are
         * Phi_i Phi_i^T + I and whose off-diagonal blocks are -Phi_{i+1}. */
        for (size_t i = 0; i < legs; i++) {
            double d[36];
            mul_transpose(&phi[i * 36], &phi[i * 36], d);
            for (int k = 0; k < 6; k++) {
                d[k * 6 + k] += 1.0;
            }

            double rhs[42];

            if (i > 0) {
                /* M = D_i + Phi_i C_{i-1}, and the right-hand side picks up
                 * Phi_i y_{i-1} for the same reason. */
                for (int row = 0; row < 6; row++) {
                    for (int col = 0; col < 6; col++) {
                        double sum = 0.0;
                        for (int k = 0; k < 6; k++) {
                            sum += phi[i * 36 + row * 6 + k]
                                   * chat[(i - 1) * 36 + k * 6 + col];
                        }
                        d[row * 6 + col] += sum;
                    }
                }

                double carried[6];
                apply(&phi[i * 36], &yhat[(i - 1) * 6], carried);
                for (int k = 0; k < 6; k++) {
                    yhat[i * 6 + k] += carried[k];
                }
            }

            /* Right-hand side: the next off-diagonal block, -Phi_{i+1}^T, and
             * the accumulated defect, solved together in one elimination. */
            for (int row = 0; row < 6; row++) {
                for (int col = 0; col < 6; col++) {
                    rhs[row * 7 + col] = (i + 1 < legs)
                        ? -phi[(i + 1) * 36 + col * 6 + row]
                        : 0.0;
                }
                rhs[row * 7 + 6] = yhat[i * 6 + row];
            }

            double solution[42];
            if (!solve6(d, rhs, 7, solution)) {
                return CORE_ERR_TOLERANCE_NOT_MET;
            }

            for (int row = 0; row < 6; row++) {
                for (int col = 0; col < 6; col++) {
                    chat[i * 36 + row * 6 + col] = solution[row * 7 + col];
                }
                yhat[i * 6 + row] = solution[row * 7 + 6];
            }
        }

        /* Back substitution gives the multipliers in place. */
        for (size_t back = legs - 1; back > 0; back--) {
            size_t i = back - 1;
            double carried[6];
            apply(&chat[i * 36], &yhat[(i + 1) * 6], carried);
            for (int k = 0; k < 6; k++) {
                yhat[i * 6 + k] -= carried[k];
            }
        }

        /* dX_i = Phi_i^T lambda_i - lambda_{i-1} - e_i, unscaled on the way
         * out. The last term is what keeps the answer near the guess. */
        for (size_t i = 0; i < n; i++) {
            double delta[6] = { 0.0, 0.0, 0.0, 0.0, 0.0, 0.0 };

            if (i < legs) {
                apply_transpose(&phi[i * 36], &yhat[i * 6], delta);
            }
            if (i > 0) {
                for (int k = 0; k < 6; k++) {
                    delta[k] -= yhat[(i - 1) * 6 + k];
                }
            }

            double here[6];
            pack(&states[i], length, speed, here);
            for (int k = 0; k < 6; k++) {
                delta[k] -= here[k] - guess[i * 6 + k];
            }

            states[i].r.x += delta[0] * length;
            states[i].r.y += delta[1] * length;
            states[i].r.z += delta[2] * length;
            states[i].v.x += delta[3] * speed;
            states[i].v.y += delta[4] * speed;
            states[i].v.z += delta[5] * speed;

            double moved = vec3_norm(vec3(delta[0], delta[1], delta[2]))
                           * length;
            if (moved > worst_step) {
                worst_step = moved;
            }
        }
    }

    report->worst_step_m = worst_step;
    return CORE_ERR_TOLERANCE_NOT_MET;
}
