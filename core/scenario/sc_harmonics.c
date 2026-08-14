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
    field.c[harmonics_index(2, 0)] = opaque(-1.08262668e-3);
    field.c[harmonics_index(2, 2)] = opaque(1.57e-6);
    field.s[harmonics_index(2, 2)] = opaque(-9.0e-7);
    field.c[harmonics_index(3, 1)] = opaque(2.19e-6);
    field.s[harmonics_index(3, 1)] = opaque(2.68e-7);
    field.c[harmonics_index(4, 3)] = opaque(-5.4e-7);
    field.s[harmonics_index(4, 3)] = opaque(1.5e-7);

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

    printf("sc_harmonics %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
