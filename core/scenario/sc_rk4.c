/* Determinism scenario: RK4 over the two-body problem.
 *
 * The first scenario that runs actual physics. Everything before it hashed
 * arithmetic; this one hashes a trajectory, so it is the canary that will
 * catch a change in the force model or the step as soon as one appears.
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

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    TwoBodyCtx ctx = { opaque(3.98600435436e14) };

    /* Eccentric rather than circular: periapsis passage is where the step is
     * worked hardest, and where a difference would show up first. */
    State s = {
        { opaque(7.0e6), opaque(0.0), opaque(0.0) },
        { opaque(0.0), opaque(9546.0), opaque(1200.0) },
        opaque(0.0),
    };

    /* Two orbits' worth of steps, hashing the full state each time. Hashing
     * every step rather than only the endpoint means a difference is recorded
     * where it happens, instead of being partly cancelled by the time the run
     * ends. */
    double step = opaque(4.0);
    for (int i = 0; i < 6000; i++) {
        State next;
        if (rk4_step(accel_two_body, &ctx, &s, step, &next) != CORE_OK) {
            return 1;
        }
        s = next;

        core_hash_f64(&h, s.r.x);
        core_hash_f64(&h, s.r.y);
        core_hash_f64(&h, s.r.z);
        core_hash_f64(&h, s.v.x);
        core_hash_f64(&h, s.v.y);
        core_hash_f64(&h, s.v.z);
    }

    /* Derived quantities too: they combine the state in ways a raw component
     * hash would not notice if two errors happened to offset. */
    core_hash_f64(&h, two_body_energy(s.r, s.v, ctx.mu));
    core_hash_f64(&h, vec3_norm(two_body_angular_momentum(s.r, s.v)));
    core_hash_f64(&h, two_body_period(s.r, s.v, ctx.mu));

    printf("sc_rk4 %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
