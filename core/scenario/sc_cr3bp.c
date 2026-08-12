/* Determinism scenario: CR3BP in the rotating frame.
 *
 * The first scenario whose acceleration depends on velocity, through the
 * Coriolis term. That matters here beyond correctness: a velocity-dependent
 * force couples the two halves of the state, so a difference in either one
 * propagates into both immediately rather than through position alone.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cr3bp.h"
#include "hash.h"
#include "integrator.h"

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

    double mu = cr3bp_mu(opaque(398600.435436), opaque(4902.800066));
    core_hash_f64(&h, mu);

    Cr3bpCtx ctx = { mu };

    /* The Lagrange points exercise the bisection, which is arithmetic the
     * runtime performs at setup and must agree on across platforms. */
    for (int p = 1; p <= 5; p++) {
        Vec3d point;
        if (cr3bp_lagrange(mu, p, &point) != CORE_OK) {
            return 1;
        }
        core_hash_f64(&h, point.x);
        core_hash_f64(&h, point.y);
        core_hash_f64(&h, point.z);
        core_hash_f64(&h, cr3bp_jacobi(point, vec3_zero(), mu));
    }

    /* An orbit that stays bounded but wanders, integrated in legs so the
     * step carried between calls is hashed too. */
    State s = {
        { opaque(0.5), opaque(0.0), opaque(0.02) },
        { opaque(0.0), opaque(0.6), opaque(0.0) },
        opaque(0.0),
    };

    Dop853Config cfg = { 0 };
    cfg.tol_m = opaque(1e-10);
    cfg.max_steps = 1000000;

    Dop853State st = { 0 };
    for (int leg = 1; leg <= 150; leg++) {
        State next;
        if (dop853_integrate(accel_cr3bp, &ctx, &s, opaque(0.4) * (double)leg,
                             &cfg, &st, &next) != CORE_OK) {
            return 1;
        }
        s = next;

        core_hash_f64(&h, s.r.x);
        core_hash_f64(&h, s.r.y);
        core_hash_f64(&h, s.r.z);
        core_hash_f64(&h, s.v.x);
        core_hash_f64(&h, s.v.y);
        core_hash_f64(&h, s.v.z);
        core_hash_f64(&h, st.h);
        core_hash_f64(&h, cr3bp_jacobi(s.r, s.v, mu));
    }

    core_hash_f64(&h, (double)st.n_accepted);
    core_hash_f64(&h, (double)st.n_rejected);

    printf("sc_cr3bp %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
