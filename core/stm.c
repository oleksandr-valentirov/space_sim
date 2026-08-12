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

static double trace(const double a[STM_SIZE])
{
    double sum = 0.0;
    for (int i = 0; i < 6; i++) {
        sum += a[i * 6 + i];
    }
    return sum;
}

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* Modulus of the larger root of L^2 - mu L + 1 = 0, for real mu.
 *
 * Below |mu| = 2 the roots are a conjugate pair on the unit circle, and the
 * orbit is stable in that direction. Above it they are real reciprocals and
 * one of them grows. */
static double modulus_from_real_invariant(double mu)
{
    double a = dabs(mu);
    if (a <= 2.0) {
        return 1.0;
    }
    return (a + sqrt(a * a - 4.0)) / 2.0;
}

CoreResult stm_monodromy_stability(const double m[STM_SIZE],
                                   StmStability *out)
{
    if (m == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    double m2[STM_SIZE], m3[STM_SIZE];
    stm_multiply(m, m, m2);
    stm_multiply(m2, m, m3);

    double p1 = trace(m);
    double p2 = trace(m2);
    double p3 = trace(m3);

    /* Newton's identities: the first three coefficients of the characteristic
     * polynomial from the first three power sums. The other three are forced
     * by the reciprocal structure and are not computed. */
    double c1 = p1;
    double c2 = (p1 * p1 - p2) / 2.0;
    double c3 = (p1 * p1 * p1 - 3.0 * p1 * p2 + 2.0 * p3) / 6.0;

    /* Dividing the polynomial by lambda^3 and substituting
     * mu = lambda + 1/lambda gives
     *
     *     mu^3 - c1 mu^2 + (c2 - 3) mu + (2 c1 - c3) = 0
     *
     * and mu = 2 is a root, which is the unit eigenvalue pair. What is left
     * after dividing it out is mu^2 + beta mu + gamma. */
    out->unit_pair_residual = 2.0 - 2.0 * c1 + 2.0 * c2 - c3;

    double beta = 2.0 - c1;
    double gamma = c2 + 1.0 - 2.0 * c1;

    double disc = beta * beta - 4.0 * gamma;

    if (disc >= 0.0) {
        double root = sqrt(disc);
        out->real_pair = 1;
        out->invariant[0] = (-beta + root) / 2.0;
        out->invariant[1] = (-beta - root) / 2.0;

        out->index[0] = dabs(out->invariant[0]) / 2.0;
        out->index[1] = dabs(out->invariant[1]) / 2.0;

        double a = modulus_from_real_invariant(out->invariant[0]);
        double b = modulus_from_real_invariant(out->invariant[1]);
        out->lambda_max = a > b ? a : b;
        return CORE_OK;
    }

    /* A complex conjugate pair of invariants, which means the four remaining
     * eigenvalues form a quadruplet lambda, 1/lambda, conj(lambda),
     * 1/conj(lambda) - off the unit circle and off the real axis. It does not
     * arise in the halo families used here, but handling it costs a complex
     * square root written out in real arithmetic, which is cheaper than a
     * branch that returns an error nobody expects. */
    double re = -beta / 2.0;
    double im = sqrt(-disc) / 2.0;

    out->real_pair = 0;
    out->invariant[0] = re;
    out->invariant[1] = im;
    out->index[0] = 0.0;
    out->index[1] = 0.0;

    /* sqrt(mu^2 - 4) with mu = re + i im. */
    double u = re * re - im * im - 4.0;
    double v = 2.0 * re * im;
    double r = sqrt(u * u + v * v);

    double sr = sqrt((r + u) / 2.0);
    double si = sqrt((r - u) / 2.0);
    if (v < 0.0) {
        si = -si;
    }

    /* The two roots are (mu +- sqrt(mu^2-4))/2 and their moduli multiply to
     * one, so the larger is the answer. */
    double ar = (re + sr) / 2.0, ai = (im + si) / 2.0;
    double br = (re - sr) / 2.0, bi = (im - si) / 2.0;

    double amod = sqrt(ar * ar + ai * ai);
    double bmod = sqrt(br * br + bi * bi);

    out->lambda_max = amod > bmod ? amod : bmod;
    return CORE_OK;
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
