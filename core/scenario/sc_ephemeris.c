/* Determinism scenario: reading the asset, and flying in it.
 *
 * This is the scenario the runtime actually depends on. Everything else in
 * this directory hashes arithmetic on numbers the scenario made up; this one
 * hashes the path a running game takes - open a shipped asset, evaluate
 * Chebyshev coefficients out of it, sum a gravity field, integrate a vessel.
 *
 * It exists because ROADMAP C4 shipped that path with no scenario covering
 * it, for a reason that turned out to be solvable: a scenario links without
 * libm and so cannot cook itself an asset. The asset is committed instead,
 * which is what PROJECT.md section 4 says should happen to assets anyway.
 * Regenerate it with `make cook`, deliberately - it changes this hash.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "field.h"
#include "frame.h"
#include "hash.h"
#include "integrator.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"
#define DAY 86400.0

#define EARTH 3
#define MOON  4

static double opaque(double x)
{
    volatile double v = x;
    return v;
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
        fprintf(stderr, "sc_ephemeris: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);

    double t_begin, t_end;
    if (eph_span(eph, &t_begin, &t_end) != CORE_OK) {
        return 1;
    }
    core_hash_f64(&h, t_begin);
    core_hash_f64(&h, t_end);
    core_hash_f64(&h, (double)eph_body_count(eph));

    /* Every body, at times that are deliberately not interval boundaries:
     * the polynomial is least accurate away from its nodes, and a platform
     * difference in the evaluation shows there first. */
    for (int body = 0; body < eph_body_count(eph); body++) {
        core_hash_f64(&h, eph_body_mu(eph, body));

        for (int k = 0; k <= 40; k++) {
            double t = t_begin
                     + (t_end - t_begin) * opaque(0.7) * (double)k / 40.0
                     + opaque(1234.5);

            State s;
            if (eph_body_state(eph, body, t, &s) != CORE_OK) {
                return 1;
            }
            hash_vec(&h, s.r);
            hash_vec(&h, s.v);
        }
    }

    /* The synodic frame: a bisection for nothing, but a chain of cross
     * products, norms and divisions that the trajectory views depend on. */
    for (int k = 0; k <= 20; k++) {
        double t = t_begin + opaque(5.0 * DAY) * (double)k;
        if (t > t_end) {
            break;
        }

        SynodicFrame f;
        if (frame_synodic(eph, EARTH, MOON, t, &f) != CORE_OK) {
            return 1;
        }

        hash_vec(&h, f.origin);
        hash_vec(&h, f.origin_rate);
        hash_vec(&h, f.x);
        hash_vec(&h, f.y);
        hash_vec(&h, f.z);
        hash_vec(&h, f.omega);
        core_hash_f64(&h, f.length);
        core_hash_f64(&h, f.length_rate);
        core_hash_f64(&h, f.rate);
        core_hash_f64(&h, f.mu);
    }

    /* And a vessel in the field of all ten bodies, integrated in legs so the
     * carried step size is hashed too. The state is near L2 and out of the
     * plane, so no component stays near zero and no symmetry hides a
     * difference. */
    FieldCtx field;
    if (field_all_bodies(eph, &field) != CORE_OK) {
        return 1;
    }

    SynodicFrame frame;
    if (frame_synodic(eph, EARTH, MOON, t_begin, &frame) != CORE_OK) {
        return 1;
    }

    State halo = {
        { opaque(1.1693640722281695), opaque(0.0), opaque(-9.6760151777927794e-02) },
        { opaque(0.0), opaque(-1.9391736078339492e-01), opaque(0.0) },
        opaque(0.0),
    };

    State vessel;
    frame_to_inertial(&frame, &halo, &vessel);
    hash_vec(&h, vessel.r);
    hash_vec(&h, vessel.v);

    Dop853Config cfg = { 0 };
    cfg.tol_m = opaque(1e-2);
    cfg.max_steps = 20000000;

    Dop853State st = { 0 };

    for (int leg = 1; leg <= 30; leg++) {
        double t = t_begin + opaque(1.5 * DAY) * (double)leg;

        State next;
        if (dop853_integrate(accel_field, &field, &vessel, t, &cfg, &st, &next)
            != CORE_OK) {
            return 1;
        }
        vessel = next;

        hash_vec(&h, vessel.r);
        hash_vec(&h, vessel.v);
        core_hash_f64(&h, st.h);

        /* Back into the rotating frame, which is what the map draws. */
        SynodicFrame now;
        if (frame_synodic(eph, EARTH, MOON, t, &now) != CORE_OK) {
            return 1;
        }
        State q;
        frame_from_inertial(&now, &vessel, &q);
        hash_vec(&h, q.r);
        hash_vec(&h, q.v);
    }

    core_hash_f64(&h, (double)st.n_accepted);
    core_hash_f64(&h, (double)st.n_rejected);
    core_hash_f64(&h, (double)field.failed);

    eph_free(eph);

    printf("sc_ephemeris %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
