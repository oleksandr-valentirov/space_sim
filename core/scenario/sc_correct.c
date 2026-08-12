/* Determinism scenario: differential correction, family continuation, and the
 * stability of each orbit found.
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

/* The monodromy matrix of an orbit and the stability that follows from it
 * (ROADMAP C3). Hashed alongside each orbit because it is a long chain of
 * arithmetic - three matrix products, Newton's identities, a quadratic - all
 * of it downstream of the corrector, so a platform difference anywhere earlier
 * arrives here amplified by the sensitivities. */
static int hash_stability(CoreHash *h, double mu, const HaloOrbit *o)
{
    Cr3bpCtx ctx = { mu };

    Dop853Config cfg;
    cfg.tol_m = 1e-13;
    cfg.h_init = 0.0;
    cfg.h_min = 0.0;
    cfg.h_max = 0.0;
    cfg.max_steps = 10000000;

    Dop853State st = { 0.0, 0, 0, 0 };
    double m[STM_SIZE];
    State end;

    if (stm_integrate(accel_cr3bp_var, &ctx, &o->s, o->period, &cfg, &st, &end,
                      m) != CORE_OK) {
        return 0;
    }

    StmStability s;
    if (stm_monodromy_stability(m, &s) != CORE_OK) {
        return 0;
    }

    core_hash_f64(h, (double)s.real_pair);
    core_hash_f64(h, s.invariant[0]);
    core_hash_f64(h, s.invariant[1]);
    core_hash_f64(h, s.index[0]);
    core_hash_f64(h, s.index[1]);
    core_hash_f64(h, s.lambda_max);
    core_hash_f64(h, s.unit_pair_residual);
    return 1;
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
    if (!hash_stability(&h, mu, &start)) {
        return 1;
    }

    HaloOrbit family[FAMILY];
    size_t count = 0;
    if (halo_family(mu, &start, opaque(0.004), &cfg, family, FAMILY, &count)
        != CORE_OK) {
        return 1;
    }
    core_hash_f64(&h, (double)count);
    for (size_t i = 0; i < count; i++) {
        hash_orbit(&h, &family[i]);
        if (!hash_stability(&h, mu, &family[i])) {
            return 1;
        }
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
        if (!hash_stability(&h, mu, &family[i])) {
            return 1;
        }
    }

    printf("sc_correct %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
