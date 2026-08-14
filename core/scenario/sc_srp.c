/* Determinism scenario: solar radiation pressure and its shadow (ROADMAP K6a).
 *
 * The interesting part to pin down is srp_acos and the branches that call it.
 * Everything up to the "fully lit" early exit is a handful of multiplications
 * that any compiler will agree on; the polynomial, the overlap area and the
 * three-way split between umbra, annular and partial are where a rearranged
 * expression would show. So the geometries below deliberately land in every
 * branch, including the two boundaries where the branches meet.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "srp.h"

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

    /* The polynomial on its own, across both halves and the reflection
     * point, so a change to it is visible here even if every shadow
     * geometry below happened to miss it. */
    for (int i = -20; i <= 20; i++) {
        core_hash_f64(&h, srp_acos(opaque((double)i / 20.0)));
    }

    double sun_r = opaque(6.957e8);
    double earth_r = opaque(6378137.0);
    double au = opaque(1.495978707e11);

    /* Vessel positions relative to a body at the origin, with the Sun far
     * out along -x. Down-axis distances: low orbit, geostationary, lunar,
     * just short of the umbra's apex and past it. Lateral offsets sweep from
     * the axis out through the penumbra into sunlight. */
    static const double DOWN[5] = { 7.0e6, 4.2164e7, 3.844e8, 1.3e9, 2.0e9 };

    SrpParams p;
    p.flux_1au = opaque(1367.6);
    p.sun_radius = sun_r;
    p.coeff = opaque(0.02);

    for (int i = 0; i < 5; i++) {
        for (int k = 0; k <= 12; k++) {
            double lat = opaque(DOWN[i] * 0.02 * (double)k);

            Vec3d v = vec3(opaque(DOWN[i]), lat, opaque(1.0e5));
            Vec3d to_sun = vec3_sub(vec3(-au, 0.0, 0.0), v);
            Vec3d to_body = vec3_neg(v);

            double f = srp_shadow(to_sun, sun_r, to_body, earth_r);
            core_hash_f64(&h, f);

            Vec3d a;
            srp_accel(&p, to_sun, f, &a);
            core_hash_f64(&h, a.x);
            core_hash_f64(&h, a.y);
            core_hash_f64(&h, a.z);

            double g[9];
            srp_gradient(&p, to_sun, f, g);
            for (int j = 0; j < 9; j++) {
                core_hash_f64(&h, g[j]);
            }
        }
    }

    printf("sc_srp %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
