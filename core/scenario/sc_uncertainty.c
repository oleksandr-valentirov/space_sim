/* Determinism scenario: covariance propagation.
 *
 * Small on purpose - the state transition matrix itself is already the
 * sharper test (sc_stm.c: seven blocks through one adaptive controller).
 * What this adds is the bookkeeping in core/uncertainty.c: the matrix
 * products of Phi P Phi^T and a mid-flight scale, chained across legs so a
 * platform difference in any of them shows up in the final hash.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cr3bp.h"
#include "hash.h"
#include "stm.h"
#include "uncertainty.h"

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

    /* Same self-contained setup as sc_stm.c: a halo-like state near L2, not
     * a catalogue orbit, so this scenario does not depend on a data file. */
    Cr3bpCtx ctx = { opaque(1.215058560962404e-02) };
    State s = {
        { opaque(1.08), opaque(0.0), opaque(-0.2) },
        { opaque(0.0), opaque(-0.2), opaque(0.0) },
        opaque(0.0),
    };

    Dop853Config cfg = { 0 };
    cfg.tol_m = opaque(1e-12);
    cfg.max_steps = 1000000;

    Dop853State st = { 0 };

    double p[STM_SIZE];
    for (int i = 0; i < STM_SIZE; i++) {
        p[i] = opaque(0.0);
    }
    p[0 * 6 + 0] = opaque(1e-8);
    p[1 * 6 + 1] = opaque(2e-8);
    p[2 * 6 + 2] = opaque(1.5e-8);
    p[3 * 6 + 3] = opaque(1e-10);
    p[4 * 6 + 4] = opaque(1e-10);
    p[5 * 6 + 5] = opaque(1e-10);

    for (int leg = 1; leg <= 20; leg++) {
        double phi[STM_SIZE];
        State next;

        if (stm_integrate(accel_cr3bp_var, &ctx, &s, opaque(0.06) * (double)leg,
                          &cfg, &st, &next, phi) != CORE_OK) {
            return 1;
        }
        s = next;

        double next_p[STM_SIZE];
        uncertainty_propagate(phi, p, next_p);
        for (int k = 0; k < STM_SIZE; k++) {
            p[k] = next_p[k];
        }

        /* A "pass" every five legs - the same mid-flight scale
         * ex_uncertainty.c uses, exercised here for the hash. */
        if (leg % 5 == 0) {
            uncertainty_scale(p, opaque(0.1));
        }

        core_hash_f64(&h, uncertainty_position_sigma(p));
        core_hash_f64(&h, uncertainty_velocity_sigma(p));
        core_hash_f64(&h, uncertainty_symmetry_defect(p));
    }

    for (int k = 0; k < STM_SIZE; k++) {
        core_hash_f64(&h, p[k]);
    }

    printf("sc_uncertainty %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
