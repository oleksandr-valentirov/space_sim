/* Determinism scenario: the propagator behind the boundary (ROADMAP H1).
 *
 * sc_ephemeris already hashes a vessel flying in the field of ten bodies, and
 * this is not a second copy of that. What it covers is the layer the game will
 * actually call through: prop_run, its samples, and the step it carries from
 * one call to the next.
 *
 * The samples are the point. They are the accepted steps of the controller, so
 * hashing them hashes the step sequence itself - not just where the trajectory
 * ended up, but every place it stopped along the way and in what order. A
 * platform that made one different decision about a rejected step would land
 * on the same endpoint to within the tolerance and disagree here.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3
#define DAY 86400.0

/* Geostationary radius and a tilted circular orbit, the same setup as
 * core/test/test_prop.c: fast enough to take real steps, and no component of
 * the state parked at zero where a difference could hide. */
#define ORBIT_R 42164.0e3

#define CAP 5

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

static void hash_state(CoreHash *h, const State *s)
{
    hash_vec(h, s->r);
    hash_vec(h, s->v);
    core_hash_f64(h, s->t);
}

/* sqrt only - the scenarios link without libm, which is the second line of
 * defence after make check-libm. */
static double root(double x)
{
    return sqrt(x);
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "sc_prop: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return 1;
    }

    double t_begin, t_span_end;
    if (eph_span(eph, &t_begin, &t_span_end) != CORE_OK) {
        return 1;
    }

    double t0 = t_begin + opaque(1.0 * DAY);

    State earth;
    if (eph_body_state(eph, EARTH, t0, &earth) != CORE_OK) {
        return 1;
    }

    double speed = root(eph_body_mu(eph, EARTH) / opaque(ORBIT_R));

    State vessel;
    vessel.r = vec3(earth.r.x + opaque(ORBIT_R), earth.r.y, earth.r.z);
    vessel.v = vec3(earth.v.x,
                    earth.v.y + opaque(0.8) * speed,
                    earth.v.z + opaque(0.6) * speed);
    vessel.t = t0;

    /* Braced, not field by field: -Wmissing-field-initializers only
     * sees this form, and PropConfig has grown a field twice now. */
    PropConfig cfg = { CORE_INTEG_DOP853, opaque(1e-2), opaque(1800.0), 0,
                       opaque(1.0) };

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);
    hash_state(&h, &vessel);

    /* Two days, in legs of five samples: the buffer decides where the run
     * stops, so the leg boundaries themselves are a product of the step
     * sequence rather than of a time chosen here. */
    double t_end = t0 + opaque(2.0 * DAY);
    double step = 0.0;
    long legs = 0;
    long total = 0;

    for (;;) {
        State samples[CAP];
        size_t n = 0;
        State final_state;
        CoreStopReason stop;
        int event = -1;

        if (prop_run(p, &vessel, NULL, t_end, NULL, 0, samples, CAP, &n, &final_state,
                     &stop, &event, &step) != CORE_OK) {
            return 1;
        }

        for (size_t i = 0; i < n; i++) {
            hash_state(&h, &samples[i]);
        }
        core_hash_f64(&h, (double)n);
        core_hash_f64(&h, step);
        core_hash_f64(&h, (double)stop);

        vessel = final_state;
        legs++;
        total += (long)n;

        if (stop == CORE_STOP_T_END) {
            break;
        }
        if (legs > 100000) {
            return 1;
        }
    }

    hash_state(&h, &vessel);
    core_hash_f64(&h, (double)legs);
    core_hash_f64(&h, (double)total);

    /* And the same propagator driven by events, on an eccentric orbit that
     * has a periapsis worth finding.
     *
     * This is the most branch-heavy code on the runtime side of the boundary:
     * how many Newton iterations the root search takes, and whether each one
     * is taken at all or replaced by a bisection, are decided by comparing
     * floating point numbers. Same reason sc_trajectory exists for multiple
     * shooting - a platform that disagreed by one ulp somewhere could take a
     * different number of iterations and land on a different last bit. */
    State ecc = vessel;
    ecc.r = vec3(earth.r.x + opaque(ORBIT_R), earth.r.y, earth.r.z);
    ecc.v = vec3(earth.v.x,
                 earth.v.y + opaque(0.8) * opaque(0.8) * speed,
                 earth.v.z + opaque(0.6) * opaque(0.8) * speed);
    ecc.t = t0;

    CoreEvent evs[2];
    evs[0].kind = CORE_EVENT_PERIAPSIS;
    evs[0].body_id = EARTH;
    evs[0].param = 0.0;
    evs[1].kind = CORE_EVENT_DISTANCE;
    evs[1].body_id = EARTH;
    evs[1].param = opaque(30000.0e3);

    step = 0.0;
    for (int i = 0; i < 12; i++) {
        State samples[CAP];
        size_t n = 0;
        State final_state;
        CoreStopReason stop;
        int event = -1;

        if (prop_run(p, &ecc, NULL, t0 + opaque(4.0 * DAY), evs, 2, samples, CAP, &n,
                     &final_state, &stop, &event, &step) != CORE_OK) {
            return 1;
        }

        hash_state(&h, &final_state);
        core_hash_f64(&h, (double)n);
        core_hash_f64(&h, (double)stop);
        core_hash_f64(&h, (double)event);
        core_hash_f64(&h, step);

        ecc = final_state;
    }

    prop_free(p);
    eph_free(eph);

    printf("sc_prop %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
