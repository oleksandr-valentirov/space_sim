#include "uncertainty.h"

#include <math.h>

static void transpose(const double a[STM_SIZE], double out[STM_SIZE])
{
    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            out[j * 6 + i] = a[i * 6 + j];
        }
    }
}

void uncertainty_propagate(const double phi[STM_SIZE], const double p[STM_SIZE],
                           double out[STM_SIZE])
{
    double phi_t[STM_SIZE];
    transpose(phi, phi_t);

    double tmp[STM_SIZE];
    stm_multiply(phi, p, tmp);   /* tmp = Phi P */
    stm_multiply(tmp, phi_t, out); /* out = (Phi P) Phi^T */
}

void uncertainty_scale(double p[STM_SIZE], double factor)
{
    for (int i = 0; i < STM_SIZE; i++) {
        p[i] *= factor;
    }
}

double uncertainty_position_sigma(const double p[STM_SIZE])
{
    double trace = p[0 * 6 + 0] + p[1 * 6 + 1] + p[2 * 6 + 2];
    return sqrt(trace / 3.0);
}

double uncertainty_velocity_sigma(const double p[STM_SIZE])
{
    double trace = p[3 * 6 + 3] + p[4 * 6 + 4] + p[5 * 6 + 5];
    return sqrt(trace / 3.0);
}

double uncertainty_symmetry_defect(const double p[STM_SIZE])
{
    double worst = 0.0;

    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            double d = p[i * 6 + j] - p[j * 6 + i];
            if (d < 0.0) {
                d = -d;
            }
            if (d > worst) {
                worst = d;
            }
        }
    }

    return worst;
}
