/* Determinism scenario: vector arithmetic.
 *
 * Kept separate from sc_arith so the two golden hashes stay independent. If
 * both move, the cause is the compiler or the flags; if only this one moves,
 * the cause is vec3. Merging them into a single hash would destroy exactly
 * that distinction.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "vec3.h"

#include <stdio.h>

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static void hash_vec3(CoreHash *h, Vec3d a)
{
    core_hash_f64(h, a.x);
    core_hash_f64(h, a.y);
    core_hash_f64(h, a.z);
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    /* A dependent chain over values that are not exactly representable, so
     * every step actually rounds. Magnitudes are deliberately spread across
     * orbital scales: 1e11 m is interplanetary distance, 1e3 m/s is orbital
     * speed. Cancellation between them is where precision is really lost. */
    Vec3d p = vec3(opaque(1.234e11), opaque(-5.678e10), opaque(9.1011e9));
    Vec3d v = vec3(opaque(-3.456e3), opaque(1.789e3), opaque(0.234e3));

    for (int i = 0; i < 2000; i++) {
        double dt = opaque(60.0);

        p = vec3_add_scaled(p, v, dt);

        /* A crude inverse-square kick. Not physics, just the shape of it:
         * the same sequence of divisions, sqrt and scaled adds the real force
         * loop will run. */
        double r2 = vec3_norm_sq(p);
        double r = vec3_norm(p);
        double f = opaque(-1.32712440018e20) / (r2 * r);

        v = vec3_add_scaled(v, p, f * dt);

        hash_vec3(&h, p);
        hash_vec3(&h, v);
    }

    core_hash_f64(&h, vec3_dot(p, v));
    hash_vec3(&h, vec3_cross(p, v));

    /* Summation order, at vector level. */
    {
        Vec3d terms[4] = {
            vec3(opaque(1.0), opaque(1e16), opaque(1e-16)),
            vec3(opaque(1e16), opaque(-1e16), opaque(1.0)),
            vec3(opaque(-1e16), opaque(1.0), opaque(-1e16)),
            vec3(opaque(1e-16), opaque(1e-16), opaque(1e16)),
        };
        hash_vec3(&h, vec3_sum_ordered(terms, 4));
    }

    printf("sc_vec3 %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
