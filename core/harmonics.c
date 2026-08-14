/* Pines' recursion (ROADMAP K1). See harmonics.h for the interface and
 * PROJECT.md section 4 for why this exists at all.
 *
 * The derivation, since none of it is quoted from a paper without checking:
 *
 * U(x,y,z) = mu * sum_n sum_m (Re/r)^n A_nm(u) [C_nm R_m(s,t) + S_nm I_m(s,t)]
 *
 * where s=x/r, t=y/r, u=z/r are direction cosines, R_m+iI_m=(s+it)^m carries
 * the longitude dependence, and A_nm(u) is the associated Legendre function
 * with its singular factor (1-u^2)^(m/2) divided out - the non-singular part
 * Pines works with. Two substitutions turn this into code:
 *
 * 1. R_m(s,t) = R_m(x,y) / r^m, where R_m(x,y)+iI_m(x,y) = (x+iy)^m is the
 *    SAME recursion run on (x,y) directly instead of (s,t). That folds the
 *    r^m into the existing r^-(n+1), leaving one power of r per term
 *    (build_ri below), not two.
 * 2. A_nm satisfies the standard triangular Legendre recursion with the
 *    (1-u^2)^(m/2) factor removed throughout (build_legendre): the sectorial
 *    term A_mm=(2m-1)!! is a genuine constant (no residual u dependence,
 *    because P_mm is proportional to (1-u^2)^(m/2) exactly), and the general
 *    branch follows by dividing the standard three-term recursion through by
 *    that same factor on all three terms, which share it.
 *
 * The gradient is the chain rule applied to that sum, term by term, using
 * ds/dx=(1-s^2)/r, du/dx=-su/r and so on, plus d(R_m+iI_m)/dx = m(R_{m-1}+
 * iI_{m-1}) and d(R_m+iI_m)/dy = im(R_{m-1}+iI_{m-1}) from differentiating
 * (x+iy)^m directly. dA_nm/du (Ad_nm here) comes from differentiating the
 * Legendre recursion itself, not a separate identity - one more reason the
 * two triangles are built together in build_legendre. The result, checked
 * against the textbook closed-form J2 acceleration for n=2, m=0 in
 * core/test/test_harmonics.c: it reproduces it exactly, sign included, which
 * is the strongest evidence this is right that does not require trusting the
 * algebra on inspection. */

#include "harmonics.h"

/* R_m + i I_m = (x + iy)^m, built on (x, y) directly - see the file comment
 * for why this is not normalised by r here. */
static void build_ri(double x, double y, int degree, double *rr, double *ii)
{
    rr[0] = 1.0;
    ii[0] = 0.0;
    for (int m = 1; m <= degree; m++) {
        rr[m] = x * rr[m - 1] - y * ii[m - 1];
        ii[m] = x * ii[m - 1] + y * rr[m - 1];
    }
}

/* A_nm(u) and its derivatives in u, built together: the general branch of
 * each needs the one below it, so splitting the triangles into separate
 * passes would mean recomputing the lower ones.
 *
 * add may be NULL - only harmonics_gradient wants the second derivative,
 * and the acceleration is the hot path. */
static void build_legendre(double u, int degree, double *a, double *ad,
                           double *add)
{
    double df = 1.0; /* running (2m-1)!!; 1 for m=0 by convention */

    for (int m = 0; m <= degree; m++) {
        if (m > 0) {
            df *= (double)(2 * m - 1);
        }
        a[harmonics_index(m, m)] = df;
        ad[harmonics_index(m, m)] = 0.0; /* sectorial term is a constant in u */
        if (add != NULL) {
            add[harmonics_index(m, m)] = 0.0;
        }

        if (m + 1 <= degree) {
            a[harmonics_index(m + 1, m)] = u * (double)(2 * m + 1) * df;
            ad[harmonics_index(m + 1, m)] = (double)(2 * m + 1) * df;
            if (add != NULL) {
                /* Linear in u, so the second derivative vanishes here too. */
                add[harmonics_index(m + 1, m)] = 0.0;
            }
        }

        for (int n = m + 2; n <= degree; n++) {
            double a1 = a[harmonics_index(n - 1, m)];
            double a2 = a[harmonics_index(n - 2, m)];
            double ad1 = ad[harmonics_index(n - 1, m)];
            double ad2 = ad[harmonics_index(n - 2, m)];

            a[harmonics_index(n, m)] =
                (u * (double)(2 * n - 1) * a1 - (double)(n + m - 1) * a2) /
                (double)(n - m);
            /* d/du of the line above, product-ruled on the u*(2n-1)*a1 term. */
            ad[harmonics_index(n, m)] =
                ((double)(2 * n - 1) * a1 + u * (double)(2 * n - 1) * ad1 -
                 (double)(n + m - 1) * ad2) /
                (double)(n - m);

            if (add != NULL) {
                /* And again, product rule on the same term: the derivative
                 * of (2n-1)*a1 + u*(2n-1)*ad1 is 2*(2n-1)*ad1 +
                 * u*(2n-1)*add1. Differentiated from the recursion rather
                 * than looked up, the same choice ad above records. */
                double add1 = add[harmonics_index(n - 1, m)];
                double add2 = add[harmonics_index(n - 2, m)];

                add[harmonics_index(n, m)] =
                    (2.0 * (double)(2 * n - 1) * ad1 +
                     u * (double)(2 * n - 1) * add1 -
                     (double)(n + m - 1) * add2) /
                    (double)(n - m);
            }
        }
    }
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
    build_ri(r.x, r.y, degree, rr, ii);

    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, NULL);

    double re_pow[HARMONICS_MAX_DEGREE + 1];
    re_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        re_pow[n] = re_pow[n - 1] * field->re;
    }

    /* Every term needs r^-(p+1) with p = n+1+m <= 2*degree+1, so the table
     * runs one further than harmonics_potential's. */
    int max_power = 2 * degree + 2;
    double r_inv_pow[2 * HARMONICS_MAX_DEGREE + 3];
    r_inv_pow[0] = 1.0;
    for (int k = 1; k <= max_power; k++) {
        r_inv_pow[k] = r_inv_pow[k - 1] * r_inv;
    }

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
            double coef = mu * re_pow[n] * r_inv_pow[p + 1];

            double dx = -(double)p * s * av * g - u * s * adv * g;
            double dy = -(double)p * t * av * g - u * t * adv * g;
            double dz = -(double)p * u * av * g + (1.0 - u * u) * adv * g;

            /* Longitude derivative, present only for m >= 1 - see the file
             * comment. At m=0, R_m/I_m are the constants 1/0 with zero
             * derivative, so this term is correctly absent, not skipped. */
            if (m > 0) {
                double hx = c * rr[m - 1] + sv * ii[m - 1];
                double hy = sv * rr[m - 1] - c * ii[m - 1];
                dx += rad * av * (double)m * hx;
                dy += rad * av * (double)m * hy;
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
    build_ri(r.x, r.y, degree, rr, ii);

    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    double add_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, add_leg);

    double re_pow[HARMONICS_MAX_DEGREE + 1];
    re_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        re_pow[n] = re_pow[n - 1] * field->re;
    }

    /* One power further than the acceleration's table: p + 2. */
    int max_power = 2 * degree + 3;
    double r_inv_pow[2 * HARMONICS_MAX_DEGREE + 4];
    r_inv_pow[0] = 1.0;
    for (int k = 1; k <= max_power; k++) {
        r_inv_pow[k] = r_inv_pow[k - 1] * r_inv;
    }

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
            double coef = mu * re_pow[n] * r_inv_pow[p + 2];

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
                      + av * (-(double)p * rad * sym_ng + rad * rad * gij)
                      + adv * rad * sym_wg;

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
    build_ri(r.x, r.y, degree, rr, ii);

    /* build_legendre also fills the derivative triangle; unused here, but
     * splitting it into two builders to save that would duplicate the whole
     * recursion for a function that is not on the hot path (see
     * harmonics.h). */
    double a_leg[HARMONICS_MAX_COEFFS];
    double ad_leg[HARMONICS_MAX_COEFFS];
    build_legendre(u, degree, a_leg, ad_leg, NULL);

    double re_pow[HARMONICS_MAX_DEGREE + 1];
    re_pow[0] = 1.0;
    for (int n = 1; n <= degree; n++) {
        re_pow[n] = re_pow[n - 1] * field->re;
    }

    int max_power = 2 * degree + 1;
    double r_inv_pow[2 * HARMONICS_MAX_DEGREE + 2];
    r_inv_pow[0] = 1.0;
    for (int k = 1; k <= max_power; k++) {
        r_inv_pow[k] = r_inv_pow[k - 1] * r_inv;
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
            int p = n + 1 + m;
            sum += re_pow[n] * r_inv_pow[p] * a_leg[idx] * g;
        }
    }

    *u_out = mu * sum;
}
