/* core/harmonics.c (ROADMAP K1): the Pines recursion, checked five ways
 * independent of each other - closed-form J2, exact rotation, finite
 * differences of the potential, and the pole's m=1-only property, which
 * falls out of the recursion itself rather than a reference. */

#include "harmonics.h"
#include "test.h"

#include <math.h>
#include <stdio.h>

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
    harmonics_set_unnormalised(&field, 2, 0, -j2, 0.0);

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
    harmonics_set_unnormalised(&field, 2, 0, -1.08262668e-3, 0.0);
    harmonics_set_unnormalised(&field, 3, 0, 2.53e-6, 0.0);
    harmonics_set_unnormalised(&field, 4, 0, 1.62e-6, 0.0);

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
    harmonics_set_unnormalised(&zonal_and_sectorial, 2, 0, -1.08e-3, 0.0);
    harmonics_set_unnormalised(&zonal_and_sectorial, 2, 2, 1.5e-6, 9.0e-7);
    harmonics_set_unnormalised(&zonal_and_sectorial, 4, 3, 3.0e-7, 0.0);

    Vec3d a;
    harmonics_accel(&zonal_and_sectorial, pole, mu, &a);
    CHECK(a.x == 0.0);
    CHECK(a.y == 0.0);

    HarmonicsField with_order_one = zonal_and_sectorial;
    harmonics_set_unnormalised(&with_order_one, 3, 1, 4.0e-7, -2.0e-7);

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
            harmonics_set_unnormalised(&field, n, m, 1.0, 0.0);

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
    harmonics_set_unnormalised(&field, 2, 0, -1.08262668e-3, 0.0);
    harmonics_set_unnormalised(&field, 2, 2, 1.57e-6, -9.0e-7);
    harmonics_set_unnormalised(&field, 3, 1, 2.19e-6, 2.68e-7);
    harmonics_set_unnormalised(&field, 4, 3, -5.4e-7, 1.5e-7);

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

/* harmonics_gradient (ROADMAP K8a), three ways.
 *
 * Central differences of harmonics_accel are the decisive one: that
 * function is already pinned to the closed-form J2 field and to the
 * textbook Legendre functions above, so agreeing with its derivative is
 * agreeing with something external. Symmetry and tracelessness cost
 * nothing and catch different mistakes - a transposed index and a wrong
 * coefficient in one of the five groups respectively. */
static void test_gradient_matches_finite_difference_of_accel(void)
{
    double mu = 3.986004418e14;

    HarmonicsField field = { 0 };
    field.degree = 4;
    field.re = 6378136.3;
    harmonics_set_unnormalised(&field, 2, 0, -1.08262668e-3, 0.0);
    harmonics_set_unnormalised(&field, 2, 2, 1.57e-6, -9.0e-7);
    harmonics_set_unnormalised(&field, 3, 1, 2.19e-6, 2.68e-7);
    harmonics_set_unnormalised(&field, 4, 3, -5.4e-7, 1.5e-7);

    Vec3d points[4] = {
        vec3(6.9e6, 1.2e6, 3.1e6),
        vec3(-2.0e6, -6.5e6, 1.0e6),
        vec3(3.0e6, 3.0e6, 6.0e6),
        vec3(0.0, 0.0, 7.2e6),          /* the pole, where A' and A'' matter */
    };

    double h = 50.0; /* metres */

    for (int p = 0; p < 4; p++) {
        Vec3d r = points[p];

        double g[9];
        harmonics_gradient(&field, r, mu, g);

        /* Symmetric to the bit, by construction rather than by rounding. */
        CHECK_BITS_EQ(g[1], g[3]);
        CHECK_BITS_EQ(g[2], g[6]);
        CHECK_BITS_EQ(g[5], g[7]);

        /* Laplace. Scaled by the size of the diagonal, since the trace is
         * a cancellation of three large numbers. */
        double trace = g[0] + g[4] + g[8];
        double scale = fabs(g[0]) + fabs(g[4]) + fabs(g[8]);
        CHECK(scale > 0.0);
        CHECK(fabs(trace) < 1e-10 * scale);

        for (int j = 0; j < 3; j++) {
            Vec3d rp = r, rm = r;
            double *pp = j == 0 ? &rp.x : (j == 1 ? &rp.y : &rp.z);
            double *pm = j == 0 ? &rm.x : (j == 1 ? &rm.y : &rm.z);
            *pp += h;
            *pm -= h;

            Vec3d ap, am;
            harmonics_accel(&field, rp, mu, &ap);
            harmonics_accel(&field, rm, mu, &am);

            double numeric[3] = {
                (ap.x - am.x) / (2.0 * h),
                (ap.y - am.y) / (2.0 * h),
                (ap.z - am.z) / (2.0 * h),
            };

            for (int i = 0; i < 3; i++) {
                double want = g[i * 3 + j];
                double got = numeric[i];
                double ref = fabs(want) > 0.0 ? fabs(want) : 1.0;
                CHECK(fabs(got - want) < 1e-6 * ref);
            }
        }
    }
}

/* A field below degree 2 has no gradient to give, and says so with zeros
 * rather than with whatever was in the caller's array. */
static void test_gradient_of_disabled_field_is_zero(void)
{
    HarmonicsField field = { 0 };
    field.degree = 0;

    double g[9];
    for (int k = 0; k < 9; k++) {
        g[k] = 1.0;
    }
    harmonics_gradient(&field, vec3(7.0e6, 1.0e6, 2.0e6), 3.986e14, g);
    for (int k = 0; k < 9; k++) {
        CHECK(g[k] == 0.0);
    }
}


/* ---- The normalised form (ROADMAP K5b) ------------------------------- */

/* What the UNNORMALISED implementation produced, at commit 9319a1d, for the
 * field and points built in test_normalised_matches_the_old_form below.
 *
 * Baked in rather than recomputed, because the code that produced them is
 * gone - that is the whole nature of this check. Ten numbers per point:
 * acceleration, potential, and the six independent entries of the Hessian.
 *
 * These are NOT a claim about correctness; the closed-form J2 test and the
 * finite-difference tests are. They are a claim about SAMENESS: the rewrite
 * was supposed to change how the sum is carried, not what it sums to. */
static const double UNNORMALISED_FORM[6][10] = {
    { -0.0016545819668486133, -0.0061372877325799428, -0.0061052368961115296, 13371.594859087543,
      -2.1941394508590488e-09, 5.1657217258174992e-09, 2.6603252096186363e-09,
      2.9110262870932189e-09, 4.0678155526450336e-09, -7.1688683623416977e-10
      },
    { 0.0035178509730011257, 0.0014757894012059742, 0.0060435803398982274, 9551.7576040487656,
      2.8527402478135204e-09, -1.9346252616601538e-10, 3.9161165277618134e-09,
      -3.2090289986553312e-09, -2.1765031769335521e-10, 3.5628875084181001e-10
      },
    { -0.003033121801647273, -0.00051128083702517532, 1.4230530750771157e-05, 9377.8659875036901,
      1.3681013632055947e-09, 4.0539743222894264e-10, -2.9191378382745903e-11,
      -5.0687092093227773e-10, -1.163534240925517e-11, -8.6123044227331683e-10
      },
    { 8.0858274875122166e-05, 9.8949852358596987e-06, 0.021934780000242682, -51181.153333899616,
      6.2761683735427416e-09, -5.2098956216951971e-12, -5.7755910625087251e-11,
      6.2579916265959382e-09, -7.0678465970426408e-12, -1.253416000013867e-08
      },
    { 0.009681309582263712, -0.0095618132973110687, 0.010788969918120445, -36986.107347124489,
      1.2408507741442235e-09, 2.4466341105702337e-09, -5.8370512108839509e-09,
      1.2820511863740919e-09, 5.6252876138264255e-09, -2.5229019605183168e-09
      },
    { 0.0081983659759825092, 0.0031382677176791259, -0.0014375668282043349, 19366.573944025655,
      4.8374254303967845e-09, 3.0732062147150269e-09, -1.2209412319613919e-09,
      -2.2038353568025608e-09, -4.2193673213054119e-10, -2.6335900735942237e-09
      },
};

static void test_normalised_matches_the_old_form(void)
{
    HarmonicsField f = { 0 };
    f.degree = 8;
    f.re = 6378137.0;
    harmonics_set_unnormalised(&f, 2, 0, -1.08262668e-3, 0.0);
    harmonics_set_unnormalised(&f, 2, 2, 1.57e-6, -9.0e-7);
    harmonics_set_unnormalised(&f, 3, 1, 2.19e-6, 2.68e-7);
    harmonics_set_unnormalised(&f, 4, 3, -5.4e-7, 1.5e-7);
    harmonics_set_unnormalised(&f, 6, 6, 2.0e-8, -3.0e-8);
    harmonics_set_unnormalised(&f, 8, 5, -7.0e-9, 4.0e-9);

    const double points[6][3] = {
        { 6.9e6, 1.1e6, 2.3e6 },
        { -4.2e6, 5.5e6, -3.1e6 },
        { 1.0e7, 0.0, 0.0 },
        { 0.0, 0.0, 7.0e6 },      /* on the pole: Pines' whole reason to exist */
        { 2.0e6, -2.0e6, 6.5e6 },
        { -8.0e6, -1.0e5, 4.0e5 },
    };

    double mu = 3.986004418e14;
    double worst = 0.0;

    for (int i = 0; i < 6; i++) {
        Vec3d r = vec3(points[i][0], points[i][1], points[i][2]);

        Vec3d a;
        double pot;
        double g[9];
        harmonics_accel(&f, r, mu, &a);
        harmonics_potential(&f, r, mu, &pot);
        harmonics_gradient(&f, r, mu, g);

        double got[10] = { a.x, a.y, a.z, pot,
                           g[0], g[1], g[2], g[4], g[5], g[8] };

        for (int k = 0; k < 10; k++) {
            double want = UNNORMALISED_FORM[i][k];
            double d = fabs(got[k] - want) / fabs(want);
            if (d > worst) {
                worst = d;
            }
        }
    }

    /* Measured 1.74e-15, which is where a difference between two orderings
     * of the same sum belongs. A bit-for-bit criterion was impossible here
     * and ROADMAP K5 says why: normalisation multiplies intermediates by
     * square roots of irrationals, so this is a different sequence of
     * operations by construction, not the same one rearranged. */
    printf("  normalised vs unnormalised form: %.3g relative\n", worst);
    CHECK(worst < 1e-13);
}

/* N_nm against its own definition, computed with factorials.
 *
 * The recursions in harmonics.c exist to avoid those factorials - (n+m)! is
 * 1e158 at n = m = 50 - so the test computes them the forbidden way at small
 * degrees, where a double still holds them exactly, and checks the two agree.
 * A test is allowed libm and lgamma; the core is not. */
static void test_normalisation_matches_factorials(void)
{
    int checked = 0;

    for (int n = 0; n <= 12; n++) {
        for (int m = 0; m <= n; m++) {
            double k = (m == 0) ? 1.0 : 2.0;
            double num = tgamma((double)(n - m) + 1.0);
            double den = tgamma((double)(n + m) + 1.0);
            double want = sqrt(num * (2.0 * (double)n + 1.0) * k / den);

            CHECK(close_rel(harmonics_normalisation(n, m), want, 1e-13));
            checked++;
        }
    }

    CHECK(checked == 91);

    /* Out of range answers zero rather than reading past the triangle. */
    CHECK(harmonics_normalisation(2, 3) == 0.0);
    CHECK(harmonics_normalisation(-1, 0) == 0.0);
}

/* Degree 50 at a lunar radius: the case that used to be NaN.
 *
 * This is the reason K5b exists, so it is checked as a property rather than
 * against a reference: every output finite, and the Hessian's trace zero,
 * which is Laplace's equation and needs no external number at all. A field
 * that overflowed anywhere in the recursion could not satisfy it. */
static void test_high_degree_stays_finite(void)
{
    HarmonicsField f = { 0 };
    f.degree = HARMONICS_MAX_DEGREE;
    f.re = 1738000.0;

    for (int n = 2; n <= f.degree; n++) {
        for (int m = 0; m <= n; m++) {
            double v = 1.0e-4 / ((double)n * (double)n);
            f.c[harmonics_index(n, m)] = (m % 2) ? -v : v;
            f.s[harmonics_index(n, m)] = (m % 3) ? 0.5 * v : -0.5 * v;
        }
    }

    double mu = 4.9028e12;

    const double points[4][3] = {
        { 1838000.0, 0.0, 0.0 },
        { 0.0, 0.0, 1838000.0 },   /* over the pole */
        { 1.0e6, -1.0e6, 1.2e6 },
        { 1738000.0, 1.0, -1.0 },  /* just off the axis, just above the surface */
    };

    double worst_trace = 0.0;

    for (int i = 0; i < 4; i++) {
        Vec3d r = vec3(points[i][0], points[i][1], points[i][2]);

        Vec3d a;
        double pot;
        double g[9];
        harmonics_accel(&f, r, mu, &a);
        harmonics_potential(&f, r, mu, &pot);
        harmonics_gradient(&f, r, mu, g);

        CHECK(isfinite(a.x) && isfinite(a.y) && isfinite(a.z));
        CHECK(isfinite(pot));
        for (int k = 0; k < 9; k++) {
            CHECK(isfinite(g[k]));
        }

        /* Something has to be there, or "finite" would be satisfied by a
         * silent zero - the failure this test would otherwise miss. */
        CHECK(vec3_norm(a) > 1e-9);

        double scale = fabs(g[0]) + fabs(g[4]) + fabs(g[8]);
        double trace = fabs(g[0] + g[4] + g[8]) / scale;
        if (trace > worst_trace) {
            worst_trace = trace;
        }
    }

    printf("  degree %d at the Moon: finite, worst relative trace %.3g\n",
           HARMONICS_MAX_DEGREE, worst_trace);
    CHECK(worst_trace < 1e-12);
}

int main(void)
{
    test_disabled_field_is_zero();
    test_gradient_of_disabled_field_is_zero();
    test_gradient_matches_finite_difference_of_accel();
    test_legendre_matches_textbook();
    test_j2_matches_closed_form();
    test_zonal_field_is_axisymmetric();
    test_pole_transverse_accel();
    test_gradient_matches_finite_difference();
    test_normalised_matches_the_old_form();
    test_normalisation_matches_factorials();
    test_high_degree_stays_finite();
    return TEST_RESULT();
}
