/* Determinism scenario: DOP853, the runtime integrator.
 *
 * The most important golden hash so far. An adaptive step sequence depends on
 * its own history, so any difference in arithmetic does not merely perturb
 * the answer - it changes which steps are taken, and the trajectories part
 * company from there. That makes this scenario the most sensitive detector of
 * a platform difference in the whole suite.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "integrator.h"

#include <stdio.h>

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static void hash_state(CoreHash *h, const State *s)
{
    core_hash_f64(h, s->r.x);
    core_hash_f64(h, s->r.y);
    core_hash_f64(h, s->r.z);
    core_hash_f64(h, s->v.x);
    core_hash_f64(h, s->v.y);
    core_hash_f64(h, s->v.z);
    core_hash_f64(h, s->t);
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    TwoBodyCtx ctx = { opaque(3.98600435436e14) };

    /* Eccentric and inclined: periapsis works the step controller hardest,
     * and a non-zero z component means no coordinate stays exactly zero. */
    State s = {
        { opaque(7.0e6), opaque(0.0), opaque(0.0) },
        { opaque(0.0), opaque(9546.0), opaque(1200.0) },
        opaque(0.0),
    };

    Dop853Config cfg = { 0 };
    cfg.tol_m = opaque(1e-6);

    /* Many short legs rather than one long integration. Each leg carries the
     * step across in Dop853State, so this exercises the continuation path
     * that saves depend on, and hashes the step size itself - a difference in
     * step selection is caught even when the state has not visibly moved. */
    Dop853State st = { 0 };
    for (int leg = 1; leg <= 200; leg++) {
        State next;
        double t_end = opaque(400.0) * (double)leg;

        if (dop853_integrate(accel_two_body, &ctx, &s, t_end, &cfg, &st,
                             &next) != CORE_OK) {
            return 1;
        }
        s = next;

        hash_state(&h, &s);
        core_hash_f64(&h, st.h);
    }

    core_hash_f64(&h, (double)st.n_accepted);
    core_hash_f64(&h, (double)st.n_rejected);
    core_hash_f64(&h, (double)st.n_evals);
    core_hash_f64(&h, two_body_energy(s.r, s.v, ctx.mu));

    printf("sc_dop853 %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
