/* core/harmonics.c (ROADMAP K1): the Pines recursion, checked five ways
 * independent of each other - closed-form J2, exact rotation, finite
 * differences of the potential, and the pole's m=1-only property, which
 * falls out of the recursion itself rather than a reference. */

#include "harmonics.h"
#include "test.h"

#include <math.h>

static int close_rel(double a, double b, double tol)
{
    double denom = fabs(b) > 0.0 ? fabs(b) : 1.0;
    return fabs(a - b) / denom < tol;
}

/* Textbook closed-form J2 acceleration (Vallado), the point-mass term
 * subtracted out - the same perturbation harmonics_accel(degree=2, only
 * C_20=-J2) is supposed to reproduce. */
static Vec3d j2_accel_reference(Vec3d r, double mu, double re, double j2)
{
    double rad = vec3_norm(r);
    double u = r.z / rad;
    double factor = -1.5 * j2 * mu * re * re / (rad * rad * rad * rad * rad);

    Vec3d a;
    a.x = factor * r.x * (1.0 - 5.0 * u * u);
    a.y = factor * r.y * (1.0 - 5.0 * u * u);
    a.z = factor * r.z * (3.0 - 5.0 * u * u);
    return a;
}

static void test_disabled_field_is_zero(void)
{
    HarmonicsField field = { 0 };
    field.degree = 1; /* below the degree-2 floor */

    Vec3d a;
    harmonics_accel(&field, vec3(7.0e6, 1.0e6, 2.0e6), 3.986e14, &a);
    CHECK(a.x == 0.0 && a.y == 0.0 && a.z == 0.0);

    double u;
    harmonics_potential(&field, vec3(7.0e6, 1.0e6, 2.0e6), 3.986e14, &u);
    CHECK(u == 0.0);
}

static void test_j2_matches_closed_form(void)
{
    double mu = 3.986004418e14;
    double re = 6378136.3;
    double j2 = 1.08262668e-3;

    HarmonicsField field = { 0 };
    field.degree = 2;
    field.re = re;
    field.c[harmonics_index(2, 0)] = -j2;

    Vec3d points[5] = {
        vec3(7.0e6, 0.0, 0.0),
        vec3(0.0, 7.0e6, 0.0),
        vec3(0.0, 0.0, 7.0e6),          /* pole */
        vec3(4.0e6, 3.0e6, 2.0e6),
        vec3(-2.0e6, 5.0e6, -3.0e6),
    };

    for (int i = 0; i < 5; i++) {
        Vec3d got;
        harmonics_accel(&field, points[i], mu, &got);
        Vec3d want = j2_accel_reference(points[i], mu, re, j2);

        CHECK(close_rel(got.x, want.x, 1e-9));
        CHECK(close_rel(got.y, want.y, 1e-9));
        CHECK(close_rel(got.z, want.z, 1e-9));
    }
}

/* A field made of zonal terms only (m=0 throughout) is axisymmetric: a 90
 * degree rotation about the pole is exact arithmetic, (x,y) -> (-y,x), no
 * trigonometry needed to state the invariant precisely. */
static void test_zonal_field_is_axisymmetric(void)
{
    HarmonicsField field = { 0 };
    field.degree = 4;
    field.re = 6378136.3;
    field.c[harmonics_index(2, 0)] = -1.08262668e-3;
    field.c[harmonics_index(3, 0)] = 2.53e-6;
    field.c[harmonics_index(4, 0)] = 1.62e-6;

    Vec3d r = vec3(5.1e6, 3.2e6, 2.7e6);
    Vec3d r_rot = vec3(-r.y, r.x, r.z);

    Vec3d a, a_rot;
    harmonics_accel(&field, r, 3.986004418e14, &a);
    harmonics_accel(&field, r_rot, 3.986004418e14, &a_rot);

    CHECK(close_rel(a_rot.x, -a.y, 1e-12));
    CHECK(close_rel(a_rot.y, a.x, 1e-12));
    CHECK(close_rel(a_rot.z, a.z, 1e-12));
}

/* Exactly on the rotation axis, only m=1 terms can produce a transverse
 * pull: R_m(0,0)=I_m(0,0)=0 for every m>=1, so the m*(...) correction that
 * carries the longitude derivative is fed R_{m-1}, I_{m-1}, which are
 * nonzero only when m-1=0. This is a property of the recursion, not of any
 * reference table, so it is an independent check on the same code path the
 * m=0 tests above do not exercise. */
static void test_pole_transverse_accel(void)
{
    double mu = 3.986004418e14;
    double re = 6378136.3;
    Vec3d pole = vec3(0.0, 0.0, 7.0e6);

    HarmonicsField zonal_and_sectorial = { 0 };
    zonal_and_sectorial.degree = 4;
    zonal_and_sectorial.re = re;
    zonal_and_sectorial.c[harmonics_index(2, 0)] = -1.08e-3;
    zonal_and_sectorial.c[harmonics_index(2, 2)] = 1.5e-6;
    zonal_and_sectorial.s[harmonics_index(2, 2)] = 9.0e-7;
    zonal_and_sectorial.c[harmonics_index(4, 3)] = 3.0e-7;

    Vec3d a;
    harmonics_accel(&zonal_and_sectorial, pole, mu, &a);
    CHECK(a.x == 0.0);
    CHECK(a.y == 0.0);

    HarmonicsField with_order_one = zonal_and_sectorial;
    with_order_one.c[harmonics_index(3, 1)] = 4.0e-7;
    with_order_one.s[harmonics_index(3, 1)] = -2.0e-7;

    harmonics_accel(&with_order_one, pole, mu, &a);
    CHECK(a.x != 0.0 || a.y != 0.0);
}

/* A_nm(u) from the textbook, with the singular (1-u^2)^(m/2) factor Pines
 * divides out already removed - i.e. P_nm(u) / (1-u^2)^(m/2), written out
 * rather than recursed, so this shares no code with build_legendre.
 *
 * Returns NAN for pairs not tabulated here, which the caller skips. */
static double legendre_a_reference(int n, int m, double u)
{
    double u2 = u * u;

    if (n == 2 && m == 0) return (3.0 * u2 - 1.0) / 2.0;
    if (n == 3 && m == 0) return (5.0 * u2 * u - 3.0 * u) / 2.0;
    if (n == 4 && m == 0) return (35.0 * u2 * u2 - 30.0 * u2 + 3.0) / 8.0;
    if (n == 5 && m == 0)
        return (63.0 * u2 * u2 * u - 70.0 * u2 * u + 15.0 * u) / 8.0;
    if (n == 6 && m == 0)
        return (231.0 * u2 * u2 * u2 - 315.0 * u2 * u2 + 105.0 * u2 - 5.0)
             / 16.0;

    if (n == 2 && m == 1) return 3.0 * u;
    if (n == 2 && m == 2) return 3.0;
    if (n == 3 && m == 1) return 1.5 * (5.0 * u2 - 1.0);
    if (n == 3 && m == 2) return 15.0 * u;
    if (n == 3 && m == 3) return 15.0;
    if (n == 4 && m == 1) return 2.5 * (7.0 * u2 * u - 3.0 * u);
    if (n == 4 && m == 2) return 7.5 * (7.0 * u2 - 1.0);
    if (n == 4 && m == 3) return 105.0 * u;
    if (n == 4 && m == 4) return 105.0;
    if (n == 5 && m == 2) return 52.5 * (3.0 * u2 * u - u);
    if (n == 5 && m == 5) return 945.0;
    if (n == 6 && m == 3) return 157.5 * (11.0 * u2 * u - 3.0 * u);

    return NAN;
}

/* Re[(x + iy)^m], the same quantity build_ri produces, computed here by
 * plain repeated complex multiplication. */
static double r_m_reference(double x, double y, int m)
{
    double re = 1.0, im = 0.0;
    for (int k = 0; k < m; k++) {
        double nr = x * re - y * im;
        double ni = x * im + y * re;
        re = nr;
        im = ni;
    }
    return re;
}

/* The recursion against the textbook, term by term.
 *
 * The closed-form J2 test above pins exactly one pair, (2, 0). Every other
 * (n, m) is checked only for internal consistency by the finite-difference
 * test below - and an error in build_legendre's general branch would move
 * the potential and its gradient together, so that test would pass while
 * both were wrong. This is the check that does not have that blind spot:
 * a single unit coefficient at a time, and the potential it produces
 * compared against
 *
 *     U = mu * Re^n * r^-(n+1+m) * A_nm(u) * R_m(x, y)
 *
 * assembled from the two reference helpers above. Degree 6 rather than 2
 * because the general branch (n > m+1) is the one with the division in it,
 * and it only starts being exercised beyond the two base cases. */
static void test_legendre_matches_textbook(void)
{
    double mu = 3.986004418e14;
    double re = 6378136.3;

    Vec3d points[3] = {
        vec3(4.0e6, 3.0e6, 2.0e6),
        vec3(-2.0e6, 5.0e6, -3.0e6),
        vec3(1.0e6, -1.5e6, 6.5e6),
    };

    int checked = 0;

    for (int n = 2; n <= 6; n++) {
        for (int m = 0; m <= n; m++) {
            if (isnan(legendre_a_reference(n, m, 0.5))) {
                continue;
            }

            HarmonicsField field = { 0 };
            field.degree = n;
            field.re = re;
            field.c[harmonics_index(n, m)] = 1.0;

            for (int p = 0; p < 3; p++) {
                Vec3d r = points[p];
                double rad = vec3_norm(r);
                double u = r.z / rad;

                double got;
                harmonics_potential(&field, r, mu, &got);

                double want = mu * pow(re, n) * pow(rad, -(n + 1 + m))
                            * legendre_a_reference(n, m, u)
                            * r_m_reference(r.x, r.y, m);

                CHECK(close_rel(got, want, 1e-12));
                checked++;
            }
        }
    }

    /* A silently empty loop would "pass" - the same failure mode
     * scripts/check_no_libm.sh guards against with its own empty check.
     * 17 tabulated (n, m) pairs at 3 points; this caught an off-by-three
     * in the first version of that count, which is the point. */
    CHECK(checked == 51);
}

/* The general (n, m) check the closed-form J2 comparison above cannot be:
 * central differences of harmonics_potential against harmonics_accel for a
 * field with tesseral terms, the same tool C2b used on the STM. */
static void test_gradient_matches_finite_difference(void)
{
    double mu = 3.986004418e14;
    HarmonicsField field = { 0 };
    field.degree = 4;
    field.re = 6378136.3;
    field.c[harmonics_index(2, 0)] = -1.08262668e-3;
    field.c[harmonics_index(2, 2)] = 1.57e-6;
    field.s[harmonics_index(2, 2)] = -9.0e-7;
    field.c[harmonics_index(3, 1)] = 2.19e-6;
    field.s[harmonics_index(3, 1)] = 2.68e-7;
    field.c[harmonics_index(4, 3)] = -5.4e-7;
    field.s[harmonics_index(4, 3)] = 1.5e-7;

    Vec3d points[3] = {
        vec3(6.9e6, 1.2e6, 3.1e6),
        vec3(-2.0e6, -6.5e6, 1.0e6),
        vec3(3.0e6, 3.0e6, 6.0e6),
    };

    double h = 1.0; /* metres; see ROADMAP K1 for the error-budget reasoning */

    for (int i = 0; i < 3; i++) {
        Vec3d r = points[i];
        Vec3d a;
        harmonics_accel(&field, r, mu, &a);

        double u_px, u_mx, u_py, u_my, u_pz, u_mz;
        harmonics_potential(&field, vec3(r.x + h, r.y, r.z), mu, &u_px);
        harmonics_potential(&field, vec3(r.x - h, r.y, r.z), mu, &u_mx);
        harmonics_potential(&field, vec3(r.x, r.y + h, r.z), mu, &u_py);
        harmonics_potential(&field, vec3(r.x, r.y - h, r.z), mu, &u_my);
        harmonics_potential(&field, vec3(r.x, r.y, r.z + h), mu, &u_pz);
        harmonics_potential(&field, vec3(r.x, r.y, r.z - h), mu, &u_mz);

        double fd_x = (u_px - u_mx) / (2.0 * h);
        double fd_y = (u_py - u_my) / (2.0 * h);
        double fd_z = (u_pz - u_mz) / (2.0 * h);

        CHECK(close_rel(a.x, fd_x, 1e-6));
        CHECK(close_rel(a.y, fd_y, 1e-6));
        CHECK(close_rel(a.z, fd_z, 1e-6));
    }
}

int main(void)
{
    test_disabled_field_is_zero();
    test_legendre_matches_textbook();
    test_j2_matches_closed_form();
    test_zonal_field_is_axisymmetric();
    test_pole_transverse_accel();
    test_gradient_matches_finite_difference();
    return TEST_RESULT();
}
