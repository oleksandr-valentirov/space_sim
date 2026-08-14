/* Determinism scenario: body orientation read out of the asset (ROADMAP K3b).
 *
 * sc_quat already hashes the arithmetic of rotating by a quaternion. This
 * hashes the path that gets one: open the shipped asset, evaluate four
 * Chebyshev channels, renormalise, and turn a body-fixed direction into an
 * inertial one - which is what the tesseral terms of K5 and the co-rotating
 * atmosphere of K7 will do on every force evaluation.
 *
 * Reading it here rather than only in a unit test is the point: the fit was
 * produced by a cooker that uses cos(), and this is the side of the boundary
 * where nothing may. If the two ever disagree it must be the asset that is
 * wrong, not the platform.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "ephemeris.h"
#include "hash.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH   3
#define MOON    4
#define JUPITER 6

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static void hash_quat(CoreHash *h, Quat q)
{
    core_hash_f64(h, q.w);
    core_hash_f64(h, q.x);
    core_hash_f64(h, q.y);
    core_hash_f64(h, q.z);
}

static void hash_vec(CoreHash *h, Vec3d v)
{
    core_hash_f64(h, v.x);
    core_hash_f64(h, v.y);
    core_hash_f64(h, v.z);
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "sc_orientation: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);

    double t_begin, t_end;
    if (eph_span(eph, &t_begin, &t_end) != CORE_OK) {
        return 1;
    }

    /* Jupiter is in the list on purpose: the asset carries no orientation
     * for it, so it must come back as the exact identity, and the identity
     * has to hash as a value like any other rather than being skipped. */
    const int bodies[3] = { EARTH, MOON, JUPITER };

    for (int i = 0; i < 3; i++) {
        for (int k = 0; k <= 200; k++) {
            /* Deliberately not on interval boundaries or fit nodes: a
             * polynomial is exact at its nodes, so hashing there would test
             * the least of it. */
            double t = t_begin
                     + (t_end - t_begin) * opaque(0.9) * (double)k / 200.0
                     + opaque(777.25);

            Quat q;
            if (eph_body_orientation(eph, bodies[i], t, &q) != CORE_OK) {
                return 1;
            }
            hash_quat(&h, q);

            /* The pole and the prime meridian as the field will ask for
             * them: body-fixed directions carried into the frame the
             * ephemeris is in. */
            hash_vec(&h, quat_rotate(q, vec3(opaque(0.0), opaque(0.0),
                                             opaque(1.0))));
            hash_vec(&h, quat_rotate(q, vec3(opaque(1.0), opaque(0.0),
                                             opaque(0.0))));

            /* And back again, which is the direction K5 actually needs: a
             * vessel's position expressed in the body's own frame. */
            Vec3d r = vec3(opaque(4.1e6), opaque(-3.7e6), opaque(2.9e6));
            hash_vec(&h, quat_rotate(quat_conjugate(q), r));
        }
    }

    eph_free(eph);

    printf("sc_orientation %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
