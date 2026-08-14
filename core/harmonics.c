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

/* A_nm(u) and Ad_nm(u) = dA_nm/du, built together: the general branch of Ad
 * needs A one degree down, not Ad, so splitting the two triangles into
 * separate passes would mean computing A twice. */
static void build_legendre(double u, int degree, double *a, double *ad)
{
    double df = 1.0; /* running (2m-1)!!; 1 for m=0 by convention */

    for (int m = 0; m <= degree; m++) {
        if (m > 0) {
            df *= (double)(2 * m - 1);
        }
        a[harmonics_index(m, m)] = df;
        ad[harmonics_index(m, m)] = 0.0; /* sectorial term is a constant in u */

        if (m + 1 <= degree) {
            a[harmonics_index(m + 1, m)] = u * (double)(2 * m + 1) * df;
            ad[harmonics_index(m + 1, m)] = (double)(2 * m + 1) * df;
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
    build_legendre(u, degree, a_leg, ad_leg);

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
    build_legendre(u, degree, a_leg, ad_leg);

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
