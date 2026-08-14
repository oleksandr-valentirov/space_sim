/* Determinism scenario: Pines' recursion (ROADMAP K1).
 *
 * Exercises harmonics_accel and harmonics_potential across degree, order and
 * position - the part of the recursion most likely to shift a bit under a
 * different compiler is the division in build_legendre's general branch, so
 * this scenario deliberately reaches degree 4 (past the two base cases) at
 * several latitudes, including the pole.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "harmonics.h"
#include "hash.h"

#include <stdio.h>

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    HarmonicsField field = { 0 };
    field.degree = 4;
    field.re = opaque(6378136.3);
    /* Through the setter since K5b: the numbers below are cited
     * unnormalised, and the struct holds normalised. Written straight into
     * c[] they would still be a field - just a different one from the one
     * this scenario has always meant, and the hash would not say so. */
    harmonics_set_unnormalised(&field, 2, 0, opaque(-1.08262668e-3), 0.0);
    harmonics_set_unnormalised(&field, 2, 2, opaque(1.57e-6), opaque(-9.0e-7));
    harmonics_set_unnormalised(&field, 3, 1, opaque(2.19e-6), opaque(2.68e-7));
    harmonics_set_unnormalised(&field, 4, 3, opaque(-5.4e-7), opaque(1.5e-7));

    double mu = opaque(3.986004418e14);

    Vec3d points[6] = {
        vec3(7.0e6, 0.0, 0.0),
        vec3(0.0, 7.0e6, 0.0),
        vec3(0.0, 0.0, 7.0e6),
        vec3(4.0e6, 3.0e6, 2.0e6),
        vec3(-2.0e6, 5.0e6, -3.0e6),
        vec3(-6.9e6, -1.1e6, 0.5e6),
    };

    for (int i = 0; i < 6; i++) {
        Vec3d a;
        harmonics_accel(&field, points[i], mu, &a);
        core_hash_f64(&h, a.x);
        core_hash_f64(&h, a.y);
        core_hash_f64(&h, a.z);

        double u;
        harmonics_potential(&field, points[i], mu, &u);
        core_hash_f64(&h, u);
    }

    /* And the degree the whole of K5b exists for. Everything above stays at
     * degree 4, where the unnormalised form also worked, so it keeps
     * comparing what it always compared; this block is the new arithmetic -
     * a full triangle of 1326 coefficients, the recursion running fifty rows
     * deep, at a lunar reference radius.
     *
     * The coefficients are generated rather than cited: no lunar data is in
     * the repository yet (K5e), and what needs pinning here is the recursion,
     * not anybody's gravity model. Magnitudes follow Kaula's rule roughly, so
     * the sum is dominated by the low degrees exactly as a real field is, and
     * the high rows still contribute above the last bit. Only + - * / here,
     * so the generator itself cannot drift between platforms. */
    HarmonicsField deep = { 0 };
    deep.degree = 50;
    deep.re = opaque(1738000.0);

    double seed = opaque(1.0e-4);
    for (int n = 2; n <= 50; n++) {
        for (int m = 0; m <= n; m++) {
            double scale = seed / ((double)n * (double)n);
            double alt = ((n + m) % 2 == 0) ? 1.0 : -1.0;
            deep.c[harmonics_index(n, m)] = alt * scale;
            deep.s[harmonics_index(n, m)] = alt * scale * 0.5;
        }
    }

    Vec3d lunar[4] = {
        vec3(1838000.0, 0.0, 0.0),
        vec3(0.0, 0.0, 1838000.0),   /* over the pole, where Pines must hold */
        vec3(1.0e6, -1.0e6, 1.2e6),
        vec3(-1.2e6, 0.9e6, -1.4e6),
    };

    double mu_moon = opaque(4.9028e12);

    for (int i = 0; i < 4; i++) {
        Vec3d a;
        harmonics_accel(&deep, lunar[i], mu_moon, &a);
        core_hash_f64(&h, a.x);
        core_hash_f64(&h, a.y);
        core_hash_f64(&h, a.z);

        double u;
        harmonics_potential(&deep, lunar[i], mu_moon, &u);
        core_hash_f64(&h, u);

        /* The Hessian too: it carries the second derivative triangle, which
         * nothing else in the scenarios reaches at this degree. */
        double g[9];
        harmonics_gradient(&deep, lunar[i], mu_moon, g);
        for (int k = 0; k < 9; k++) {
            core_hash_f64(&h, g[k]);
        }
    }

    printf("sc_harmonics %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
