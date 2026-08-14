/* Determinism scenario: quaternion rotation (ROADMAP K3).
 *
 * All-inline, like vec3.h - sc_vec3.c is the precedent for exercising a
 * header with no translation unit of its own directly, rather than waiting
 * for a runtime caller to appear.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "quat.h"

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

    Quat q = quat_normalize((Quat){ opaque(0.4), opaque(-0.3), opaque(0.7),
                                    opaque(0.2) });
    Vec3d v = vec3(opaque(1.234e7), opaque(-5.678e6), opaque(9.101e5));

    /* A chain of rotate/renormalize, the same shape a fitted-then-evaluated
     * quaternion sees at runtime: never exactly unit length, corrected each
     * time it is used. */
    for (int i = 0; i < 500; i++) {
        v = quat_rotate(q, v);
        hash_vec3(&h, v);

        /* Perturb q slightly and renormalize, so the chain does not just
         * repeat the same rotation 500 times - that would hash one
         * operation's determinism, not the sequence a real orientation
         * (pole precessing, meridian spinning) puts it through. */
        Quat perturbed = { q.w, q.x + opaque(1e-4), q.y - opaque(0.5e-4),
                           q.z + opaque(0.7e-4) };
        q = quat_normalize(perturbed);
        core_hash_f64(&h, q.w);
        core_hash_f64(&h, q.x);
        core_hash_f64(&h, q.y);
        core_hash_f64(&h, q.z);
    }

    Quat back = quat_conjugate(q);
    hash_vec3(&h, quat_rotate(back, v));

    printf("sc_quat %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
