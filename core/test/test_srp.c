/* Solar radiation pressure and its shadow (ROADMAP K6a).
 *
 * Two oracles, because the module makes two separate approximations and a
 * single check would let one hide behind the other.
 *
 * 1. srp_acos against libm's acos. That one is a pure question of numerics:
 *    how far is our polynomial from the function it approximates. Tests may
 *    link libm; core/srp.c may not, and `make check-libm` is what enforces
 *    that rather than this file.
 *
 * 2. srp_shadow against the EXACT spherical geometry, integrated here.
 *    srp.c models the two discs as planar circles and takes their overlap
 *    area, which is the standard model (Montenbruck and Gill 3.4) and is
 *    still a model. The exact answer is the overlap of two spherical caps,
 *    and that is computable to quadrature accuracy: ring by ring across the
 *    solar disc, the occulted range of azimuth follows from the spherical
 *    law of cosines in closed form, so only one dimension needs integrating.
 *
 *    This is the check that says how good the model is, not merely how
 *    faithfully it was typed in. A comparison against the same planar
 *    formula written twice would have said nothing about it at all. */

#include "srp.h"
#include "test.h"

#include <math.h>

#define SUN_R    6.957e8        /* data/horizons/obj_sun.txt, IAU 2015 */
#define EARTH_R  6378137.0      /* data/horizons/obj_earth.txt, equatorial */
#define MOON_R   1737400.0      /* data/horizons/obj_moon.txt, IAU */
#define AU       1.495978707e11
#define FLUX     1367.6         /* data/horizons/obj_sun.txt */

/* Spelled out here as it is in core/srp.c: PI is a POSIX extension and the
 * core compiles as strict C11. */
#define PI 3.14159265358979323846

/* ---- oracle 1: the exact spherical shadow ------------------------------- */

/* Fraction of the solar disc visible, integrated over the disc rather than
 * approximated by a planar overlap.
 *
 * For a direction at angle r from the Sun's centre and azimuth phi about it,
 * the spherical law of cosines gives the angle to the body's centre:
 *
 *     cos(angle) = cos(c) cos(r) + sin(c) sin(r) cos(phi)
 *
 * and the direction is blocked when that exceeds cos(b). So per ring the
 * blocked azimuth fraction is acos(X)/pi with X the value of cos(phi) at
 * equality - exact, no sampling. What is left is a one-dimensional integral
 * with weight sin(r), done by Simpson. */
static double exact_shadow(Vec3d to_sun, double rs, Vec3d to_body, double rb)
{
    double ds = vec3_norm(to_sun);
    double db = vec3_norm(to_body);

    if (db <= rb) {
        return 0.0;
    }
    if (vec3_dot(to_sun, to_body) <= 0.0) {
        return 1.0;
    }

    double a = asin(rs / ds);
    double b = asin(rb / db);

    double cc = vec3_dot(to_sun, to_body) / (ds * db);
    if (cc > 1.0) {
        cc = 1.0;
    }
    if (cc < -1.0) {
        cc = -1.0;
    }
    double c = acos(cc);

    if (c >= a + b) {
        return 1.0;
    }
    if (c + a <= b) {
        return 0.0;
    }

    const int N = 20000;                 /* even, for Simpson */
    double h = a / (double)N;
    double num = 0.0;
    double den = 0.0;

    for (int i = 0; i <= N; i++) {
        double r = (double)i * h;
        double w = (i == 0 || i == N) ? 1.0 : ((i % 2) ? 4.0 : 2.0);
        double sr = sin(r);

        double blocked = 0.0;
        if (c <= 0.0) {
            /* Concentric: the body's disc blocks everything inside its own
             * angular radius and nothing outside it. Worth writing rather
             * than skipping - the first version of this oracle guarded the
             * division by leaving blocked at zero here, which turned every
             * on-axis annular geometry into "fully lit" and made the model
             * look wrong by 85 per cent. */
            blocked = (r < b) ? 1.0 : 0.0;
        } else if (sr > 0.0) {
            double x = (cos(b) - cos(c) * cos(r)) / (sin(c) * sr);
            if (x > 1.0) {
                x = 1.0;
            }
            if (x < -1.0) {
                x = -1.0;
            }
            blocked = acos(x) / PI;
        }

        num += w * sr * blocked;
        den += w * sr;
    }

    return 1.0 - num / den;
}

/* ---- geometry helper ---------------------------------------------------- */

/* The Sun sits at -sun_distance on x, the occulting body at the origin, so
 * the shadow axis runs along +x. A vessel at (down_axis, lateral, 0) is then
 * down_axis metres behind the body and lateral metres off its axis. */
static void geometry(double sun_distance, double down_axis, double lateral,
                     Vec3d *to_sun, Vec3d *to_body)
{
    Vec3d v = vec3(down_axis, lateral, 0.0);
    *to_sun = vec3_sub(vec3(-sun_distance, 0.0, 0.0), v);
    *to_body = vec3_sub(vec3(0.0, 0.0, 0.0), v);
}

int main(void)
{
    /* ---- srp_acos against libm ------------------------------------------ */

    double acos_worst = 0.0;
    double acos_worst_x = 0.0;
    const int NA = 2000001;
    for (int i = 0; i < NA; i++) {
        double x = -1.0 + 2.0 * (double)i / (double)(NA - 1);
        double e = fabs(srp_acos(x) - acos(x));
        if (e > acos_worst) {
            acos_worst = e;
            acos_worst_x = x;
        }
    }
    printf("  acos: max error %.3e rad at x = %.6f\n", acos_worst, acos_worst_x);
    CHECK(acos_worst < 3.0e-8);

    /* The endpoints are where a wrong reflection would show, and they are
     * exact rather than approximate: the sqrt(1-a) factor is exactly zero at
     * a = 1, and the reflection through pi carries that to -1. */
    CHECK_BITS_EQ(srp_acos(1.0), 0.0);
    CHECK(fabs(srp_acos(-1.0) - PI) < 1.0e-15);
    CHECK(fabs(srp_acos(0.0) - PI / 2.0) < 3.0e-8);

    /* Out of range is clamped, not NaN: every caller feeds it a ratio that
     * can leave [-1, 1] by an ulp. */
    CHECK_BITS_EQ(srp_acos(1.0 + 1.0e-9), 0.0);
    CHECK(fabs(srp_acos(-1.5) - PI) < 1.0e-15);

    /* ---- the cases that must be exactly lit or exactly dark -------------- */

    Vec3d to_sun, to_body;

    /* A body with no radius in the asset cannot occult anything. */
    geometry(AU, 7.0e6, 0.0, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, 0.0), 1.0);

    /* Body on the sunward side of the vessel: nothing is between us. */
    geometry(AU, -7.0e6, 0.0, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, EARTH_R), 1.0);

    /* Straight down the shadow axis, well inside the umbra. */
    geometry(AU, 7.0e6, 0.0, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, EARTH_R), 0.0);

    /* Far off to the side, in full sunlight. */
    geometry(AU, 7.0e6, 5.0e7, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, EARTH_R), 1.0);

    /* Inside the body. Nonsense geometry, but the answer must not be a
     * plausible-looking sunlit one. */
    geometry(AU, 1.0e6, 0.0, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, EARTH_R), 0.0);

    /* ---- the umbra ends where the cone says it does --------------------- */

    /* Apex distance behind the body: R_b * d / (R_s - R_b). For the Earth at
     * one astronomical unit that is 1.384e9 m, about 3.6 lunar distances -
     * which is why an eclipse of the Moon is total and one of a spacecraft
     * beyond the Moon never is. */
    double apex = EARTH_R * AU / (SUN_R - EARTH_R);
    printf("  umbra length behind the Earth: %.4e m\n", apex);
    CHECK(fabs(apex - 1.384e9) < 0.01e9);

    geometry(AU, apex * 0.9, 0.0, &to_sun, &to_body);
    CHECK_BITS_EQ(srp_shadow(to_sun, SUN_R, to_body, EARTH_R), 0.0);

    geometry(AU, apex * 1.1, 0.0, &to_sun, &to_body);
    double annular = srp_shadow(to_sun, SUN_R, to_body, EARTH_R);
    CHECK(annular > 0.0);
    CHECK(annular < 0.2);
    printf("  visible fraction 10%% past the apex: %.6f\n", annular);

    /* ---- the model against exact spherical geometry --------------------- */

    /* Altitudes chosen to span the regime where the approximation is least
     * comfortable (a vessel just above the surface sees the Earth as a
     * 66-degree disc, not a small one) through to where both discs are
     * small. */
    static const double DOWN[] = {
        7.0e6, 1.0e7, 4.2164e7, 3.844e8, 1.0e9, 1.3e9, 1.5e9, 5.0e9,
    };
    const int ND = (int)(sizeof DOWN / sizeof DOWN[0]);

    double model_worst = 0.0;
    double model_worst_down = 0.0;
    double model_worst_lat = 0.0;
    int    swept = 0;

    for (int i = 0; i < ND; i++) {
        double down = DOWN[i];

        /* Sweep the lateral offset across the whole penumbra: the outer edge
         * sits at roughly R_b + down * (R_s + R_b) / d. Two thousand steps
         * to 1.5 times that, so the sweep starts on the axis and ends in
         * full sunlight. */
        double edge = EARTH_R + down * (SUN_R + EARTH_R) / AU;

        for (int k = 0; k <= 2000; k++) {
            double lat = 1.5 * edge * (double)k / 2000.0;
            geometry(AU, down, lat, &to_sun, &to_body);

            double got = srp_shadow(to_sun, SUN_R, to_body, EARTH_R);
            double want = exact_shadow(to_sun, SUN_R, to_body, EARTH_R);
            double e = fabs(got - want);

            swept++;
            if (e > model_worst) {
                model_worst = e;
                model_worst_down = down;
                model_worst_lat = lat;
            }
        }
    }

    printf("  shadow model vs exact spherical: %.3e over %d geometries\n",
           model_worst, swept);
    printf("    worst at %.4e m behind the body, %.4e m off axis\n",
           model_worst_down, model_worst_lat);

    /* Measured at 1.26e-4, and the bound is set just above it so that a
     * model that gets worse fails here rather than passing quietly.
     *
     * Where the worst case lands is itself the confirmation: 7000 km from
     * the centre, 6.38e6 m off axis - a vessel just above the surface with
     * the Sun's disc sitting exactly on the limb, which is precisely where
     * treating a 66-degree spherical cap as a flat circle is least
     * defensible. Everywhere both discs are small it is orders better; down
     * the Moon's shadow it is 5e-7. */
    CHECK(model_worst < 2.0e-4);

    /* The Moon too, since its disc is the one that produces annular
     * geometry at ordinary distances. */
    double moon_worst = 0.0;
    for (int k = 0; k <= 4000; k++) {
        double down = 2.0e6 + 4.0e8 * (double)k / 4000.0;
        geometry(AU, down, 0.3 * MOON_R, &to_sun, &to_body);
        double e = fabs(srp_shadow(to_sun, SUN_R, to_body, MOON_R)
                        - exact_shadow(to_sun, SUN_R, to_body, MOON_R));
        if (e > moon_worst) {
            moon_worst = e;
        }
    }
    printf("  same, down the Moon's shadow: %.3e\n", moon_worst);
    CHECK(moon_worst < 1.0e-6);

    /* ---- the fraction is monotone across the penumbra ------------------- */

    /* Not a restatement of the comparison above: a function can track an
     * oracle to a thousandth and still wobble, and a wobbling shadow gives
     * an integrator a non-physical acceleration to chase. */
    double prev = 0.0;
    int    increases = 0;
    int    reached_one = 0;
    for (int k = 0; k <= 5000; k++) {
        double lat = 1.5 * (EARTH_R + 7.0e6 * (SUN_R + EARTH_R) / AU)
                     * (double)k / 5000.0;
        geometry(AU, 7.0e6, lat, &to_sun, &to_body);
        double f = srp_shadow(to_sun, SUN_R, to_body, EARTH_R);
        if (k > 0 && f < prev) {
            increases++;
        }
        if (f == 1.0) {
            reached_one = 1;
        }
        prev = f;
    }
    CHECK(increases == 0);
    CHECK(reached_one == 1);

    /* ---- the acceleration ----------------------------------------------- */

    SrpParams p;
    p.flux_1au = FLUX;
    p.sun_radius = SUN_R;
    p.coeff = 0.02;   /* Cr * A / m: a 1000 kg vessel with 20 m^2 at Cr = 1 */

    Vec3d a;
    Vec3d sunward = vec3(AU, 0.0, 0.0);   /* the Sun is at +x from the vessel */
    srp_accel(&p, sunward, 1.0, &a);

    /* Hand computation: 1367.6 / 299792458 = 4.5618e-6 N/m^2 at one AU,
     * times 0.02 m^2/kg. */
    double want_mag = FLUX / SRP_C_M_S * p.coeff;
    printf("  a_srp at 1 AU, Cr*A/m = %.3g: %.6e m/s^2\n", p.coeff,
           vec3_norm(a));
    CHECK(fabs(vec3_norm(a) - want_mag) < 1.0e-18);

    /* Away from the Sun, and along the line to it. Compared by value rather
     * than by bits: scaling a zero component by a negative number gives -0.0,
     * which is the same number and a different bit pattern. */
    CHECK(a.x < 0.0);
    CHECK(a.y == 0.0);
    CHECK(a.z == 0.0);

    /* Inverse square. */
    Vec3d a2;
    srp_accel(&p, vec3(2.0 * AU, 0.0, 0.0), 1.0, &a2);
    CHECK(fabs(vec3_norm(a2) * 4.0 / vec3_norm(a) - 1.0) < 1.0e-14);

    /* Shadow scales it linearly, and a shadow of zero is exactly zero -
     * not a small number that keeps pushing a vessel that is in the dark. */
    Vec3d ah, az;
    srp_accel(&p, sunward, 0.5, &ah);
    srp_accel(&p, sunward, 0.0, &az);
    CHECK(fabs(vec3_norm(ah) * 2.0 / vec3_norm(a) - 1.0) < 1.0e-15);
    CHECK(az.x == 0.0);
    CHECK(az.y == 0.0);
    CHECK(az.z == 0.0);

    /* ---- the gradient --------------------------------------------------- */

    /* Finite differences of srp_accel itself, the same instrument K8a used on
     * the Pines Hessian. The shadow is held at 1 on both sides, which is
     * exactly the term the gradient claims to describe. */
    Vec3d base = vec3(0.7 * AU, 0.2 * AU, -0.1 * AU);
    double g[9];
    srp_gradient(&p, base, 1.0, g);

    double grad_worst = 0.0;
    double step = 1.0e6;
    for (int j = 0; j < 3; j++) {
        /* The vessel moves by +step along axis j, so to_sun moves by -step. */
        Vec3d plus = base;
        Vec3d minus = base;
        double *pj = (j == 0) ? &plus.x : (j == 1) ? &plus.y : &plus.z;
        double *mj = (j == 0) ? &minus.x : (j == 1) ? &minus.y : &minus.z;
        *pj -= step;
        *mj += step;

        Vec3d ap, am;
        srp_accel(&p, plus, 1.0, &ap);
        srp_accel(&p, minus, 1.0, &am);

        double fd[3] = {
            (ap.x - am.x) / (2.0 * step),
            (ap.y - am.y) / (2.0 * step),
            (ap.z - am.z) / (2.0 * step),
        };
        for (int i = 0; i < 3; i++) {
            double e = fabs(g[i * 3 + j] - fd[i]);
            double scale = fabs(g[i * 3 + j]) + 1.0e-30;
            if (e / scale > grad_worst) {
                grad_worst = e / scale;
            }
        }
    }
    printf("  gradient vs finite differences: %.3e relative\n", grad_worst);
    CHECK(grad_worst < 1.0e-8);

    /* Symmetric to the bit, as the upper-triangle-then-mirror construction
     * makes it. */
    CHECK_BITS_EQ(g[1], g[3]);
    CHECK_BITS_EQ(g[2], g[6]);
    CHECK_BITS_EQ(g[5], g[7]);

    /* ---- the term the gradient deliberately drops ------------------------ */

    /* srp.h claims the shadow's own derivative is a spike too sharp for a
     * linearisation to carry, and that the claim is measured. Here is the
     * measurement: the largest d(shadow)/dx across a low-orbit penumbra,
     * times the SRP acceleration, against the smooth term kept above and
     * against the Earth's own gravity gradient at the same place. */
    double shadow_slope = 0.0;
    double d = 1.0e3;
    for (int k = 0; k <= 20000; k++) {
        double lat = 1.5 * (EARTH_R + 7.0e6 * (SUN_R + EARTH_R) / AU)
                     * (double)k / 20000.0;
        Vec3d sp, bp, sm, bm;
        geometry(AU, 7.0e6, lat + d, &sp, &bp);
        geometry(AU, 7.0e6, lat - d, &sm, &bm);
        double slope = (srp_shadow(sp, SUN_R, bp, EARTH_R)
                        - srp_shadow(sm, SUN_R, bm, EARTH_R)) / (2.0 * d);
        if (fabs(slope) > shadow_slope) {
            shadow_slope = fabs(slope);
        }
    }

    double a_srp = FLUX / SRP_C_M_S * p.coeff;
    double dropped = shadow_slope * a_srp;
    double kept = fabs(g[0]);
    double earth_gg = 2.0 * 3.986004418e14 / (7.0e6 * 7.0e6 * 7.0e6);

    printf("  shadow gradient term %.3e 1/s^2, smooth SRP term %.3e, "
           "Earth point mass %.3e\n", dropped, kept, earth_gg);

    /* The dropped term is six orders LARGER than the smooth one that is
     * kept - which is worth knowing and is not a reason to keep it, because
     * the number that decides is the third: it is under a millionth of the
     * gravity gradient at the same place, and it only exists inside a
     * penumbra a vessel crosses in seconds. */
    CHECK(dropped > kept);
    CHECK(dropped < 1.0e-5 * earth_gg);

    return TEST_RESULT();
}
