#include "stm.h"

void stm_identity(double phi[STM_SIZE])
{
    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            phi[i * 6 + j] = (i == j) ? 1.0 : 0.0;
        }
    }
}

void stm_multiply(const double a[STM_SIZE], const double b[STM_SIZE],
                  double c[STM_SIZE])
{
    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            double sum = 0.0;
            for (int k = 0; k < 6; k++) {
                sum += a[i * 6 + k] * b[k * 6 + j];
            }
            c[i * 6 + j] = sum;
        }
    }
}

/* J is the symplectic form: J = [[0, I], [-I, 0]] in this state ordering, so
 * (J x)_i is x_{i+3} for i < 3 and -x_{i-3} otherwise. Written out rather than
 * built as a matrix, because a 6x6 of mostly zeros obscures what it does. */
double stm_symplectic_defect(const double phi[STM_SIZE])
{
    double worst = 0.0;

    for (int i = 0; i < 6; i++) {
        for (int j = 0; j < 6; j++) {
            /* (Phi^T J Phi)_ij = sum_k Phi_ki (J Phi)_kj */
            double sum = 0.0;
            for (int k = 0; k < 3; k++) {
                sum += phi[k * 6 + i] * phi[(k + 3) * 6 + j];
            }
            for (int k = 3; k < 6; k++) {
                sum -= phi[k * 6 + i] * phi[(k - 3) * 6 + j];
            }

            double expected;
            if (i < 3 && j == i + 3) {
                expected = 1.0;
            } else if (i >= 3 && j == i - 3) {
                expected = -1.0;
            } else {
                expected = 0.0;
            }

            double d = sum - expected;
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

/* Column j of the STM lives in block j+1, as a (position, velocity) pair. */
static void write_column(Vec3d *r, Vec3d *v, int j, Vec3d dr, Vec3d dv)
{
    r[j + 1] = dr;
    v[j + 1] = dv;
}

CoreResult stm_integrate(BlockAccelFunc f, void *ctx, const State *in,
                         double t_end, const Dop853Config *cfg,
                         Dop853State *io, State *out, double phi[STM_SIZE])
{
    if (f == NULL || in == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    Vec3d r[STM_BLOCKS];
    Vec3d v[STM_BLOCKS];

    r[0] = in->r;
    v[0] = in->v;

    /* Phi(t0) = I, expressed as six perturbations: unit displacement in each
     * of the three position components, then in each velocity component. */
    for (int j = 0; j < 3; j++) {
        write_column(r, v, j,
                     vec3(j == 0 ? 1.0 : 0.0,
                          j == 1 ? 1.0 : 0.0,
                          j == 2 ? 1.0 : 0.0),
                     vec3_zero());
        write_column(r, v, j + 3,
                     vec3_zero(),
                     vec3(j == 0 ? 1.0 : 0.0,
                          j == 1 ? 1.0 : 0.0,
                          j == 2 ? 1.0 : 0.0));
    }

    CoreResult res = dop853_integrate_blocks(f, ctx, STM_BLOCKS,
                                             in->t, t_end, r, v, cfg, io);
    if (res != CORE_OK) {
        return res;
    }

    out->r = r[0];
    out->v = v[0];
    out->t = t_end;

    if (phi != NULL) {
        for (int j = 0; j < 6; j++) {
            phi[0 * 6 + j] = r[j + 1].x;
            phi[1 * 6 + j] = r[j + 1].y;
            phi[2 * 6 + j] = r[j + 1].z;
            phi[3 * 6 + j] = v[j + 1].x;
            phi[4 * 6 + j] = v[j + 1].y;
            phi[5 * 6 + j] = v[j + 1].z;
        }
    }

    return CORE_OK;
}
