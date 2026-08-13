#include "target.h"

#include "stm.h"

#include <string.h>

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* Solve a x = b for a 3x3 a, Gaussian elimination with partial pivoting.
 * a and b are destroyed. Returns 0 if a is singular.
 *
 * Pivoting matters here rather than being routine: a is d(arrival position) /
 * d(departure velocity), and over a horizon of one or two revolutions its
 * three directions can differ in sensitivity by the eigenvalue of the orbit -
 * a factor of hundreds near an unstable one (core/test/test_stability.c). */
static int solve3(double a[9], double b[3], double out[3])
{
    int row[3] = { 0, 1, 2 };

    for (int col = 0; col < 3; col++) {
        int best = col;
        double best_size = dabs(a[row[col] * 3 + col]);
        for (int k = col + 1; k < 3; k++) {
            double size = dabs(a[row[k] * 3 + col]);
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

        double pivot = a[row[col] * 3 + col];

        for (int k = col + 1; k < 3; k++) {
            double factor = a[row[k] * 3 + col] / pivot;
            for (int j = col; j < 3; j++) {
                a[row[k] * 3 + j] -= factor * a[row[col] * 3 + j];
            }
            b[row[k]] -= factor * b[row[col]];
        }
    }

    for (int col = 2; col >= 0; col--) {
        double sum = b[row[col]];
        for (int j = col + 1; j < 3; j++) {
            sum -= a[row[col] * 3 + j] * out[j];
        }
        out[col] = sum / a[row[col] * 3 + col];
    }

    return 1;
}

CoreResult target_hit(BlockAccelFunc f, void *ctx,
                      const Dop853Config *integrator_cfg,
                      State *state, double t_aim, Vec3d aim,
                      double tol_m, int max_iterations,
                      TargetReport *report)
{
    if (f == NULL || integrator_cfg == NULL || state == NULL
        || !(t_aim > state->t) || !(tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }
    if (max_iterations <= 0) {
        max_iterations = 10;
    }

    int iter;
    double miss_m = 0.0;

    for (iter = 0; iter < max_iterations; iter++) {
        double phi[STM_SIZE];
        State arrival;
        Dop853State st;
        memset(&st, 0, sizeof st);

        CoreResult r = stm_integrate(f, ctx, state, t_aim, integrator_cfg,
                                     &st, &arrival, phi);
        if (r != CORE_OK) {
            if (report != NULL) {
                report->iterations = iter;
                report->miss_m = miss_m;
            }
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        Vec3d miss = vec3_sub(aim, arrival.r);
        miss_m = vec3_norm(miss);
        if (miss_m < tol_m) {
            if (report != NULL) {
                report->iterations = iter;
                report->miss_m = miss_m;
            }
            return CORE_OK;
        }

        double a[9];
        for (int row = 0; row < 3; row++) {
            for (int col = 0; col < 3; col++) {
                a[row * 3 + col] = phi[row * 6 + (col + 3)];
            }
        }

        double b[3] = { miss.x, miss.y, miss.z };
        double delta[3];
        if (!solve3(a, b, delta)) {
            if (report != NULL) {
                report->iterations = iter;
                report->miss_m = miss_m;
            }
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        state->v.x += delta[0];
        state->v.y += delta[1];
        state->v.z += delta[2];
    }

    if (report != NULL) {
        report->iterations = iter;
        report->miss_m = miss_m;
    }
    return CORE_ERR_TOLERANCE_NOT_MET;
}
