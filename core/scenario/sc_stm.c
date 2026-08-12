/* Determinism scenario: the state transition matrix.
 *
 * Worth its own scenario rather than folding into sc_cr3bp, because it
 * exercises something none of the others do: seven blocks stepped together
 * through one adaptive controller. The step sequence is now decided by block
 * zero while six more blocks ride on it, so a platform difference in the
 * controller shows up multiplied by the sensitivities instead of only in the
 * trajectory - which makes this the sharper of the two tests.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cr3bp.h"
#include "hash.h"
#include "stm.h"

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

    /* The mass ratio of the JPL halo catalogue, written out rather than
     * derived, for the reason given in core/test/test_halo.c. */
    Cr3bpCtx ctx = { opaque(1.215058560962404e-02) };

    /* A halo-like state near L2, out of the plane. Not a catalogue orbit: a
     * determinism scenario must not depend on a data file. */
    State s = {
        { opaque(1.08), opaque(0.0), opaque(-0.2) },
        { opaque(0.0), opaque(-0.2), opaque(0.0) },
        opaque(0.0),
    };

    Dop853Config cfg = { 0 };
    cfg.tol_m = opaque(1e-12);
    cfg.max_steps = 1000000;

    Dop853State st = { 0 };

    /* In legs, so the step carried between calls is part of the hash, and so
     * the composition of transition matrices is exercised as well. */
    double phi_total[STM_SIZE];
    stm_identity(phi_total);

    for (int leg = 1; leg <= 40; leg++) {
        double phi_leg[STM_SIZE];
        State next;

        if (stm_integrate(accel_cr3bp_var, &ctx, &s, opaque(0.06) * (double)leg,
                          &cfg, &st, &next, phi_leg) != CORE_OK) {
            return 1;
        }
        s = next;

        double product[STM_SIZE];
        stm_multiply(phi_leg, phi_total, product);
        for (int k = 0; k < STM_SIZE; k++) {
            phi_total[k] = product[k];
        }

        core_hash_f64(&h, s.r.x);
        core_hash_f64(&h, s.r.y);
        core_hash_f64(&h, s.r.z);
        core_hash_f64(&h, s.v.x);
        core_hash_f64(&h, s.v.y);
        core_hash_f64(&h, s.v.z);
        core_hash_f64(&h, st.h);

        for (int k = 0; k < STM_SIZE; k++) {
            core_hash_f64(&h, phi_leg[k]);
        }
    }

    for (int k = 0; k < STM_SIZE; k++) {
        core_hash_f64(&h, phi_total[k]);
    }

    double canonical[STM_SIZE];
    cr3bp_stm_canonical(phi_total, canonical);
    for (int k = 0; k < STM_SIZE; k++) {
        core_hash_f64(&h, canonical[k]);
    }
    core_hash_f64(&h, stm_symplectic_defect(canonical));

    core_hash_f64(&h, (double)st.n_accepted);
    core_hash_f64(&h, (double)st.n_rejected);

    printf("sc_stm %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
