/* Pines' recursion, fully normalised (ROADMAP K1, normalised in K5b). See
 * harmonics.h for the interface and PROJECT.md section 4 for why this exists.
 *
 * The derivation, since none of it is quoted from a paper without checking:
 *
 * U(x,y,z) = mu/r * sum_n (Re/r)^n sum_m A_nm(u) [C_nm R_m(s,t) + S_nm I_m(s,t)]
 *
 * where s=x/r, t=y/r, u=z/r are direction cosines, R_m+iI_m=(s+it)^m carries
 * the longitude dependence, and A_nm(u) is the associated Legendre function
 * with its singular factor (1-u^2)^(m/2) divided out - the non-singular part
 * Pines works with. A_nm and the coefficients are both fully normalised, and
 * only their product appears, so the physics is unchanged by that choice.
 *
 * WHAT NORMALISATION BUYS, and it is not accuracy. The sum above is written
 * in quantities that are all bounded: (Re/r)^n <= 1 outside the reference
 * sphere, |R_m|,|I_m| <= 1 because they are powers of a unit vector, and
 * normalised A_nm grow slowly enough to stay inside a double to degree 50 and
 * far beyond. The unnormalised form of this same sum multiplies Re^n by
 * r^-(n+m+1) by (2m-1)!!, each of which overflows on its own around degree
 * 44-50, and only their product is small. That is why K5b had to come before
 * any lunar data (harmonics.h).
 *
 * Two substitutions turn the sum into code:
 *
 * 1. R_m(s,t) is built on the direction cosines directly. An earlier version
 *    built it on (x,y), which folds one power of r per term into the existing
 *    r^-(n+1) and saves a multiply - and is exactly the trick that made the
 *    intermediates unbounded. The cheap form was affordable only while the
 *    degree was small.
 * 2. A_nm satisfies the standard triangular Legendre recursion with the
 *    (1-u^2)^(m/2) factor removed throughout (build_legendre): the sectorial
 *    term is a genuine constant in u (no residual u dependence, because P_mm
 *    is proportional to (1-u^2)^(m/2) exactly), and the general branch
 *    follows by dividing the three-term recursion through by that same
 *    factor, which all three terms share. Normalising multiplies each branch
 *    by a ratio of N_nm, worked out in build_legendre where it is used.
 *
 * The gradient is the chain rule applied to that sum, term by term, using
 * ds/dx=(1-s^2)/r, du/dx=-su/r and so on, plus d(R_m+iI_m)/ds = m(R_{m-1}+
 * iI_{m-1}) and the matching one in t. dA_nm/du (Ad_nm here) comes from
 * differentiating the Legendre recursion itself, not a separate identity -
 * one more reason the two triangles are built together in build_legendre.
 * The result, checked against the textbook closed-form J2 acceleration for
 * n=2, m=0 in core/test/test_harmonics.c: it reproduces it exactly, sign
 * included, which is the strongest evidence this is right that does not
 * require trusting the algebra on inspection.
 *
 * NOTE ON r < Re. Inside the reference sphere (Re/r)^n grows instead of
 * shrinking, and at degree 50 it overflows below about a fifth of Re. The
 * series does not converge there either - it is an exterior solution - so
 * this is a domain limit rather than a defect of the arithmetic, and it is
 * far inside any body a vessel can fly around. */

#include "harmonics.h"

/* R_m + i I_m = (s + it)^m on the direction cosines, so every entry has
 * modulus at most one - see the file comment for why that matters. */
static void build_ri(double s, double t, int degree, double *rr, double *ii)
{
    rr[0] = 1.0;
    ii[0] = 0.0;
    for (int m = 1; m <= degree; m++) {
        rr[m] = s * rr[m - 1] - t * ii[m - 1];
        ii[m] = s * ii[m - 1] + t * rr[m - 1];
    }
}

/* Normalised A_nm(u) and its derivatives in u, built together: the general
 * branch of each needs the one below it, so splitting the triangles into
 * separate passes would mean recomputing the lower ones.
 *
 * The three branches, with N_nm as in harmonics.h and k_m = 2 - delta_0m:
 *
 *   sectorial   A_mm     = A_{m-1,m-1} * sqrt( (2m+1) k_m / (2m k_{m-1}) )
 *   first below A_{n,n-1} = u * sqrt(2n+1) * A_{n-1,n-1}
 *   general     A_nm     = alpha * u * A_{n-1,m} - beta * A_{n-2,m}
 *
 *       alpha = sqrt( (2n+1)(2n-1) / ((n-m)(n+m)) )
 *       beta  = sqrt( (2n+1)(n+m-1)(n-m-1) / ((2n-3)(n+m)(n-m)) )
 *
 * Each is the unnormalised branch multiplied by the ratio of the two N_nm it
 * connects, with the factorials cancelled by hand before any of them is
 * evaluated - which is the point: N_nm alone underflows past degree 40, while
 * every ratio above stays within a factor of a few of one.
 *
 * The derivatives are the same three lines differentiated in u, exactly as
 * the unnormalised version did it. A_mm is constant in u and A_{n,n-1} is
 * linear, so their second derivatives vanish outright.
 *
 * add may be NULL - only harmonics_gradient wants the second derivative, and
 * the acceleration is the hot path. */
static void build_legendre(double u, int degree, double *a, double *ad,
                           double *add)
{
    a[harmonics_index(0, 0)] = 1.0;
    ad[harmonics_index(0, 0)] = 0.0;
    if (add != NULL) {
        add[harmonics_index(0, 0)] = 0.0;
    }

    for (int m = 0; m <= degree; m++) {
        if (m > 0) {
            /* k_m / k_{m-1} is 2 when leaving m = 0 and 1 afterwards. */
            double k_ratio = (m == 1) ? 2.0 : 1.0;
            double f = sqrt((double)(2 * m + 1) * k_ratio / (double)(2 * m));
            a[harmonics_index(m, m)] = f * a[harmonics_index(m - 1, m - 1)];
            ad[harmonics_index(m, m)] = 0.0;
            if (add != NULL) {
                add[harmonics_index(m, m)] = 0.0;
            }
        }

        if (m + 1 <= degree) {
            double f = sqrt((double)(2 * m + 3));
            a[harmonics_index(m + 1, m)] = u * f * a[harmonics_index(m, m)];
            ad[harmonics_index(m + 1, m)] = f * a[harmonics_index(m, m)];
            if (add != NULL) {
                add[harmonics_index(m + 1, m)] = 0.0;
            }
        }

        for (int n = m + 2; n <= degree; n++) {
            double dn = (double)n;
            double dm = (double)m;

            double alpha = sqrt((2.0 * dn + 1.0) * (2.0 * dn - 1.0) /
                                ((dn - dm) * (dn + dm)));
            double beta = sqrt((2.0 * dn + 1.0) * (dn + dm - 1.0) *
                               (dn - dm - 1.0) /
                               ((2.0 * dn - 3.0) * (dn + dm) * (dn - dm)));

            double a1 = a[harmonics_index(n - 1, m)];
            double a2 = a[harmonics_index(n - 2, m)];
            double ad1 = ad[harmonics_index(n - 1, m)];
            double ad2 = ad[harmonics_index(n - 2, m)];

            a[harmonics_index(n, m)] = alpha * u * a1 - beta * a2;
            /* d/du of the line above, product rule on alpha*u*a1. */
            ad[harmonics_index(n, m)] =
                alpha * (a1 + u * ad1) - beta * ad2;

            if (add != NULL) {
                double add1 = add[harmonics_index(n - 1, m)];
                double add2 = add[harmonics_index(n - 2, m)];
                add[harmonics_index(n, m)] =
                    alpha * (2.0 * ad1 + u * add1) - beta * add2;
            }
        }
    }
}

double harmonics_normalisation(int n, int m)
{
    if (n < 0 || m < 0 || m > n) {
        return 0.0;
    }

    /* Down the diagonal to (m, m), then up the column to (n, m). Both
     * factors are ratios of consecutive N, so nothing large is ever formed:
     *
     *   N_mm / N_{m-1,m-1} = sqrt( (2m+1) k_m / ((2m-1) k_{m-1} 2m (2m-1)) )
     *   N_nm / N_{n-1,m}   = sqrt( (n-m)(2n+1) / ((n+m)(2n-1)) )
     */
    double value = 1.0;

    for (int i = 1; i <= m; i++) {
        double di = (double)i;
        double k_ratio = (i == 1) ? 2.0 : 1.0;
        value *= sqrt((2.0 * di + 1.0) * k_ratio /
                      ((2.0 * di - 1.0) * 2.0 * di * (2.0 * di - 1.0)));
    }

    for (int i = m + 1; i <= n; i++) {
        double di = (double)i;
        double dm = (double)m;
        value *= sqrt((di - dm) * (2.0 * di + 1.0) /
                      ((di + dm) * (2.0 * di - 1.0)));
    }

    return value;
}

void harmonics_set_unnormalised(HarmonicsField *field, int n, int m,
                                double c, double s)
{
    if (field == NULL || n < 0 || m < 0 || m > n ||
        n > HARMONICS_MAX_DEGREE) {
        return;
    }

    double norm = harmonics_normalisation(n, m);
    if (!(norm > 0.0)) {
        return;
    }

    field->c[harmonics_index(n, m)] = c / norm;
    field->s[harmonics_index(n, m)] = s / norm;
}

static int clamp_degree(int degree)
{
    return degree > HARMONICS_MAX_DEGREE ? HARMONICS_MAX_DEGREE : degree;
}

void harmonics_accel(const HarmonicsField *field, Vec3d r, double mu,
                     Vec3d *a_out)
{
    *a_out = vec3_zero();

    int degree = field->degree;
    if (degree < 2) {
        return;
    }
    degree = clamp_degree(degree);

    double rad = vec3_norm(r);
    double r_inv = 1.0 / rad;
    double s = r.x * r_inv;
    double t = r.y * r_inv;
    double u = r.z * r_inv;

    double rr[HARMONICS_MAX_DEGREE + 1];
    double ii[HARMONICS_MAX_DEGREE + 1];
    build_ri(s, t, degree, rr, ii);

    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, NULL);

    /* One table instead of the two the unnormalised form needed: with R_m
     * built on the direction cosines, every term carries exactly (Re/r)^n
     * and a single mu/r^2 out front. */
    double rho = field->re * r_inv;
    double rho_pow[HARMONICS_MAX_DEGREE + 1];
    rho_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        rho_pow[n] = rho_pow[n - 1] * rho;
    }

    double outer = mu * r_inv * r_inv;

    Vec3d acc = vec3_zero();

    for (int n = 2; n <= degree; n++) {
        for (int m = 0; m <= n; m++) {
            int idx = harmonics_index(n, m);
            double c = field->c[idx];
            double sv = field->s[idx];
            if (c == 0.0 && sv == 0.0) {
                continue;
            }

            double av = a_leg[idx];
            double adv = ad_leg[idx];
            double g = c * rr[m] + sv * ii[m];

            int p = n + 1 + m;
            double coef = outer * rho_pow[n];

            double dx = -(double)p * s * av * g - u * s * adv * g;
            double dy = -(double)p * t * av * g - u * t * adv * g;
            double dz = -(double)p * u * av * g + (1.0 - u * u) * adv * g;

            /* Longitude derivative, present only for m >= 1 - see the file
             * comment. At m=0, R_m/I_m are the constants 1/0 with zero
             * derivative, so this term is correctly absent, not skipped.
             * The factor of r the unnormalised form carried here cancelled
             * against the one folded into R_m(x,y); with R_m on direction
             * cosines there is nothing left to cancel. */
            if (m > 0) {
                double hx = c * rr[m - 1] + sv * ii[m - 1];
                double hy = sv * rr[m - 1] - c * ii[m - 1];
                dx += av * (double)m * hx;
                dy += av * (double)m * hy;
            }

            acc.x += coef * dx;
            acc.y += coef * dy;
            acc.z += coef * dz;
        }
    }

    *a_out = acc;
}

/* Second derivatives of the same sum (ROADMAP K8a).
 *
 * Written per term as K r^-(p+2) times a bracket, exactly as the first
 * derivatives are K r^-(p+1) times theirs. Differentiating
 *
 *     dT/dx_i = K r^-(p+1) [ -p n_i A g + A' w_i g + r A g_i ]
 *
 * once more and collecting by which factor of A survives gives five
 * groups, where n_i = x_i/r are the direction cosines, w_i = d_iz - u n_i
 * (so that r du/dx_i = w_i), g_i = dg/dx_i and G_ij = d^2 g/dx_i dx_j:
 *
 *   A   g   [ p(p+2) n_i n_j - p d_ij ]
 *   A'  g   [ -(p+1)(n_i w_j + n_j w_i) - u(d_ij - n_i n_j) ]
 *   A'' g   [ w_i w_j ]
 *   A       [ -p r (n_i g_j + n_j g_i) + r^2 G_ij ]
 *   A'      [ r (w_i g_j + w_j g_i) ]
 *
 * Every group is symmetric in i and j by its own shape, which is the first
 * reason to arrange it this way: the symmetry of the result is a property
 * of the derivation rather than something enforced afterwards. The second
 * is that each group is homogeneous of the same degree in r, so a term
 * that came out with the wrong power of r shows up as a dimensional
 * mismatch rather than as a small error.
 *
 * Checked three ways in core/test/test_harmonics.c, and the third is the
 * one that would catch an algebra slip the other two could share: against
 * central differences of harmonics_accel, which is itself already pinned
 * to the closed-form J2 field. The other two - symmetry, and a trace that
 * vanishes because every solid harmonic satisfies Laplace's equation away
 * from its source - are free and need no reference at all. */
void harmonics_gradient(const HarmonicsField *field, Vec3d r, double mu,
                        double g_out[9])
{
    for (int k = 0; k < 9; k++) {
        g_out[k] = 0.0;
    }

    int degree = field->degree;
    if (degree < 2) {
        return;
    }
    degree = clamp_degree(degree);

    double rad = vec3_norm(r);
    double r_inv = 1.0 / rad;
    double u = r.z * r_inv;

    double dir[3] = { r.x * r_inv, r.y * r_inv, u };
    double w[3] = { -u * dir[0], -u * dir[1], 1.0 - u * u };

    double rr[HARMONICS_MAX_DEGREE + 1];
    double ii[HARMONICS_MAX_DEGREE + 1];
    build_ri(dir[0], dir[1], degree, rr, ii);

    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    double add_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, add_leg);

    double rho = field->re * r_inv;
    double rho_pow[HARMONICS_MAX_DEGREE + 1];
    rho_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        rho_pow[n] = rho_pow[n - 1] * rho;
    }

    /* One power of r further out than the acceleration, and the r and r^2
     * that used to sit inside the bracket are gone with the same
     * cancellation the acceleration records. */
    double outer = mu * r_inv * r_inv * r_inv;

    for (int n = 2; n <= degree; n++) {
        for (int m = 0; m <= n; m++) {
            int idx = harmonics_index(n, m);
            double c = field->c[idx];
            double sv = field->s[idx];
            if (c == 0.0 && sv == 0.0) {
                continue;
            }

            double av = a_leg[idx];
            double adv = ad_leg[idx];
            double addv = add_leg[idx];
            double gv = c * rr[m] + sv * ii[m];

            int p = n + 1 + m;
            double coef = outer * rho_pow[n];

            /* dg/dx_i: nonzero only in x and y, and only from m >= 1. */
            double gi[3] = { 0.0, 0.0, 0.0 };
            if (m > 0) {
                double h1 = c * rr[m - 1] + sv * ii[m - 1];
                double h2 = sv * rr[m - 1] - c * ii[m - 1];
                gi[0] = (double)m * h1;
                gi[1] = (double)m * h2;
            }

            /* d^2 g/dx_i dx_j, from m >= 2. Harmonic in two dimensions -
             * G_xx + G_yy is identically zero - which is where the trace
             * check below gets part of its bite. */
            double g_xx = 0.0, g_xy = 0.0, g_yy = 0.0;
            if (m > 1) {
                double k1 = c * rr[m - 2] + sv * ii[m - 2];
                double k2 = sv * rr[m - 2] - c * ii[m - 2];
                double mm = (double)m * (double)(m - 1);
                g_xx = mm * k1;
                g_xy = mm * k2;
                g_yy = -mm * k1;
            }

            /* Upper triangle only, mirrored after the loop. Not for speed:
             * n_i w_j + n_j w_i evaluated in the two orders differs in the
             * last bit, and a caller entitled to a symmetric matrix would
             * be quietly wrong - the same reasoning field_gradient records
             * for its own outer product. */
            for (int i = 0; i < 3; i++) {
                for (int j = i; j < 3; j++) {
                    double d_ij = (i == j) ? 1.0 : 0.0;

                    double sym_nw = dir[i] * w[j] + dir[j] * w[i];
                    double sym_ng = dir[i] * gi[j] + dir[j] * gi[i];
                    double sym_wg = w[i] * gi[j] + w[j] * gi[i];

                    double gij = 0.0;
                    if (i == 0 && j == 0) {
                        gij = g_xx;
                    } else if (i == 0 && j == 1) {
                        gij = g_xy;
                    } else if (i == 1 && j == 1) {
                        gij = g_yy;
                    }

                    double term =
                        av * gv * ((double)p * (double)(p + 2) * dir[i] * dir[j]
                                   - (double)p * d_ij)
                      + adv * gv * (-(double)(p + 1) * sym_nw
                                    - u * (d_ij - dir[i] * dir[j]))
                      + addv * gv * w[i] * w[j]
                      + av * (-(double)p * sym_ng + gij)
                      + adv * sym_wg;

                    g_out[i * 3 + j] += coef * term;
                }
            }
        }
    }

    g_out[3] = g_out[1];
    g_out[6] = g_out[2];
    g_out[7] = g_out[5];
}

void harmonics_potential(const HarmonicsField *field, Vec3d r, double mu,
                         double *u_out)
{
    *u_out = 0.0;

    int degree = field->degree;
    if (degree < 2) {
        return;
    }
    degree = clamp_degree(degree);

    double rad = vec3_norm(r);
    double r_inv = 1.0 / rad;
    double u = r.z * r_inv;

    double rr[HARMONICS_MAX_DEGREE + 1];
    double ii[HARMONICS_MAX_DEGREE + 1];
    build_ri(r.x * r_inv, r.y * r_inv, degree, rr, ii);

    /* build_legendre also fills the derivative triangle; unused here, but
     * splitting it into two builders to save that would duplicate the whole
     * recursion for a function that is not on the hot path (see
     * harmonics.h). */
    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, NULL);

    double rho = field->re * r_inv;
    double rho_pow[HARMONICS_MAX_DEGREE + 1];
    rho_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        rho_pow[n] = rho_pow[n - 1] * rho;
    }

    double sum = 0.0;

    for (int n = 2; n <= degree; n++) {
        for (int m = 0; m <= n; m++) {
            int idx = harmonics_index(n, m);
            double c = field->c[idx];
            double sv = field->s[idx];
            if (c == 0.0 && sv == 0.0) {
                continue;
            }

            double g = c * rr[m] + sv * ii[m];
            sum += rho_pow[n] * a_leg[idx] * g;
        }
    }

    *u_out = mu * r_inv * sum;
}
