/* Determinism scenario: differential correction and family continuation.
 *
 * The most demanding scenario in the set, and deliberately so. The others hash
 * a trajectory; this one hashes the result of a search, in which a branch is
 * taken on a floating-point comparison at every step - which Newton step to
 * accept, whether a root bracket has flipped sign, when to stop iterating. A
 * platform that differs in the last bit of one crossing time does not produce
 * a slightly different answer here, it produces a different number of
 * iterations, and the hash says so immediately.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "correct.h"
#include "hash.h"

#include <stdio.h>

#define FAMILY 12

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static void hash_orbit(CoreHash *h, const HaloOrbit *o)
{
    core_hash_f64(h, o->s.r.x);
    core_hash_f64(h, o->s.r.y);
    core_hash_f64(h, o->s.r.z);
    core_hash_f64(h, o->s.v.x);
    core_hash_f64(h, o->s.v.y);
    core_hash_f64(h, o->s.v.z);
    core_hash_f64(h, o->period);
    core_hash_f64(h, o->jacobi);
    core_hash_f64(h, o->residual);
    core_hash_f64(h, (double)o->iterations);
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    double mu = opaque(1.215058560962404e-02);

    HaloCorrectConfig cfg;
    cfg.hold = HALO_HOLD_Z;
    cfg.tol = opaque(1e-11);
    cfg.integrator_tol = opaque(1e-13);
    cfg.max_iterations = 30;
    cfg.max_step = opaque(0.05);

    /* Round numbers, so the answer is the corrector's and not the seed's. */
    State seed = {
        { opaque(1.10), opaque(0.0), opaque(-0.19) },
        { opaque(0.0), opaque(-0.18), opaque(0.0) },
        opaque(0.0),
    };

    HaloOrbit start;
    if (halo_correct(mu, &seed, opaque(2.5), &cfg, &start) != CORE_OK) {
        return 1;
    }
    hash_orbit(&h, &start);

    HaloOrbit family[FAMILY];
    size_t count = 0;
    if (halo_family(mu, &start, opaque(0.004), &cfg, family, FAMILY, &count)
        != CORE_OK) {
        return 1;
    }
    core_hash_f64(&h, (double)count);
    for (size_t i = 0; i < count; i++) {
        hash_orbit(&h, &family[i]);
    }

    /* The other free-variable choice reaches different orbits by a different
     * route, so it is worth hashing separately rather than assuming the first
     * covers it. */
    cfg.hold = HALO_HOLD_X;
    count = 0;
    if (halo_family(mu, &start, opaque(-0.004), &cfg, family, FAMILY, &count)
        != CORE_OK) {
        return 1;
    }
    core_hash_f64(&h, (double)count);
    for (size_t i = 0; i < count; i++) {
        hash_orbit(&h, &family[i]);
    }

    printf("sc_correct %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
