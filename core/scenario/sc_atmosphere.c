/* Determinism scenario: atmospheric density and drag (ROADMAP K7a).
 *
 * Two things here could differ between platforms if anything were free to be
 * rearranged, and neither is exercised by the rest of the core.
 *
 * The series in atmosphere_exp_neg is the obvious one: seventeen fused
 * multiply-adds waiting to happen, followed by up to six squarings that
 * amplify whatever the last bits did. So it is hashed on its own, across the
 * whole reduced range and past the cutoff, before anything calls it.
 *
 * The band search is the quiet one. It is a comparison against a table, and a
 * vessel sitting on a join is a comparison whose answer decides which
 * exponential it flies through - the same class of thing as the root finder in
 * core/prop.c, and the reason the altitudes below land exactly on band bases
 * as well as between them.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "atmosphere.h"
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

    /* The series alone. Past 64 the answer is exactly zero and the loop keeps
     * going anyway, so that the cutoff itself is pinned rather than assumed. */
    for (int i = 0; i <= 140; i++) {
        core_hash_f64(&h, atmosphere_exp_neg(opaque((double)i * 0.5)));
    }

    /* Fine steps through the first reduction seam, at x = 1, where a halving
     * appears and the squaring with it. */
    for (int i = -8; i <= 8; i++) {
        core_hash_f64(&h, atmosphere_exp_neg(opaque(1.0 + (double)i * 1e-3)));
    }

    const AtmosphereModel *m = &ATMOSPHERE_EARTH_USSA76;

    /* Every band base exactly, then a metre either side of it: three
     * comparisons that a reordered search would answer differently. */
    for (int i = 0; i < m->n_layers; i++) {
        double base = m->layer[i].base_altitude_m;
        for (int k = -1; k <= 1; k++) {
            double rho, drho;
            atmosphere_density(m, opaque(base + (double)k), &rho, &drho);
            core_hash_f64(&h, rho);
            core_hash_f64(&h, drho);
        }
    }

    /* And a sweep that does not care about bands: below the surface, through
     * the whole profile, out past the top where the model runs to zero. */
    for (int i = -20; i <= 300; i++) {
        double rho, drho;
        atmosphere_density(m, opaque((double)i * 1.0e4), &rho, &drho);
        core_hash_f64(&h, rho);
        core_hash_f64(&h, drho);
    }

    /* The force and both Jacobians, over speeds from a standing start to
     * re-entry and altitudes from the stratosphere to well above the table.
     * The velocity is skew to the vertical on purpose: parallel or
     * perpendicular would zero terms that should be pinned. */
    static const double ALT[6] = {
        3.0e4, 1.2e5, 2.6e5, 4.05e5, 7.5e5, 1.4e6
    };
    static const double SPEED[4] = { 0.0, 300.0, 7700.0, 11000.0 };

    for (int i = 0; i < 6; i++) {
        double rho, drho;
        atmosphere_density(m, opaque(ALT[i]), &rho, &drho);

        Vec3d up = vec3(opaque(0.6), opaque(0.48), opaque(0.64));

        for (int k = 0; k < 4; k++) {
            Vec3d v = vec3(opaque(-0.37 * SPEED[k]),
                           opaque(0.83 * SPEED[k]),
                           opaque(0.42 * SPEED[k]));

            Vec3d a;
            drag_accel(rho, opaque(0.004), v, &a);
            core_hash_f64(&h, a.x);
            core_hash_f64(&h, a.y);
            core_hash_f64(&h, a.z);

            double dadr[9], dadv[9];
            drag_jacobian(rho, drho, opaque(0.004), v, up, dadr, dadv);
            for (int j = 0; j < 9; j++) {
                core_hash_f64(&h, dadr[j]);
                core_hash_f64(&h, dadv[j]);
            }
        }
    }

    printf("sc_atmosphere %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
