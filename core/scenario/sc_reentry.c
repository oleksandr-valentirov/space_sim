/* Determinism scenario: stopping at an altitude (ROADMAP K7c).
 *
 * sc_dragflight pins the force the air applies; this pins the other thing K7
 * added, which is a run that ends where the caller asked rather than when the
 * step controller happened to finish. The two are deliberately separate: this
 * hash must move when the root finder moves and stay put when the atmosphere
 * table is retuned, and one scenario carrying both could not say which.
 *
 * WHAT IS ACTUALLY BEING PINNED IS THE ROOT, not the trajectory. The event
 * state comes out of a Newton search inside an accepted step, run to the point
 * where the bracket stops shrinking - about as far as double precision goes -
 * and every iteration of it re-integrates a short leg. If any of that is
 * reassociated by a compiler, or if the search takes a different number of
 * steps on another platform, this hash says so and nothing else in the set
 * would.
 *
 * The event is armed at 200 km, which is a BASE of the USSA-76 table
 * (core/atmosphere.c) and therefore exactly where the density is
 * discontinuous. That is the opposite of what sc_dragflight does, and on
 * purpose: the density jump must not reach the root finder, because the event
 * function is a distance and reads no density at all. If that ever stops
 * being true, this is where it shows.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3
#define DAY 86400.0

/* Apoapsis 400 km up, periapsis 150 km: a descent that crosses the event
 * altitude on its way down, in one third of a revolution. */
#define APO_ALT 400.0e3
#define PERI_ALT 150.0e3
#define EVENT_ALT 200.0e3
#define CAP 5

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

static double root(double x)
{
    return sqrt(x);
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

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "sc_reentry: cannot read %s\n", ASSET);
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

    /* Every radius here is built from the asset's own mean radius, which is
     * also what the event measures from. A number typed in would be a second
     * opinion about the size of the Earth. */
    double surface = eph_body_radius(eph, EARTH);
    double r_apo = surface + opaque(APO_ALT);
    double r_peri = surface + opaque(PERI_ALT);

    /* Same skew geometry as sc_dragflight, and for the same reason: no
     * component of the state or of the relative wind sits at zero, where a
     * difference could hide. */
    Vec3d dir = vec3(opaque(0.62), opaque(0.55), opaque(0.42));
    Vec3d u = vec3_scale(dir, 1.0 / root(vec3_norm_sq(dir)));

    Vec3d east = vec3_cross(vec3(0.0, 0.0, 1.0), u);
    east = vec3_scale(east, 1.0 / root(vec3_norm_sq(east)));
    Vec3d north = vec3_cross(u, east);
    Vec3d along = vec3_add_scaled(east, north, opaque(0.6));
    along = vec3_scale(along, 1.0 / root(vec3_norm_sq(along)));

    /* Vis-viva at apoapsis of the ellipse through both radii, with sqrt as
     * the only function - this links without libm on purpose. */
    double speed = root(eph_body_mu(eph, EARTH) * 2.0 * r_peri
                        / (r_apo * (r_apo + r_peri)));

    State vessel;
    vessel.r = vec3_add_scaled(earth.r, u, r_apo);
    vessel.v = vec3_add_scaled(earth.v, along, speed);
    vessel.t = t0;

    VesselParams blunt;
    blunt.mass_kg = opaque(1000.0);
    blunt.area_m2 = opaque(20.0);
    blunt.cr = opaque(0.0);
    blunt.cd = opaque(2.2);

    /* Braced, not field by field: -Wmissing-field-initializers only
     * sees this form, and PropConfig has grown a field twice now. */
    PropConfig cfg = { CORE_INTEG_DOP853, opaque(1e-2), opaque(30.0), 0,
                       opaque(1.0) };

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 1;
    }

    CoreEvent down;
    down.kind = CORE_EVENT_ALTITUDE;
    down.body_id = EARTH;
    down.param = opaque(EVENT_ALT);

    CoreHash h;
    core_hash_init(&h);
    hash_state(&h, &vessel);

    /* Long enough to reach the event with room to spare, so that a run ending
     * at t_end is a failure of the event and not of the clock. */
    double t_end = t0 + opaque(3000.0);
    double step = 0.0;
    long legs = 0;
    CoreStopReason stop = CORE_STOP_T_END;

    for (;;) {
        State samples[CAP];
        size_t n = 0;
        State final_state;
        int event = -1;

        if (prop_run(p, &vessel, &blunt, t_end, &down, 1, samples, CAP, &n,
                     &final_state, &stop, &event, &step) != CORE_OK) {
            return 1;
        }

        for (size_t i = 0; i < n; i++) {
            hash_state(&h, &samples[i]);
        }
        core_hash_f64(&h, (double)n);
        core_hash_f64(&h, step);
        core_hash_f64(&h, (double)event);

        vessel = final_state;
        legs++;

        if (stop != CORE_STOP_BUFFER_FULL) {
            break;
        }
        if (legs > 100000) {
            return 1;
        }
    }

    /* A run that ended any other way is a failed scenario, not a different
     * hash. Hashing alone would report "something changed" for a scenario
     * that had quietly stopped testing anything - the same trap as a GPU test
     * that passes on a blank frame. */
    if (stop != CORE_STOP_EVENT) {
        fprintf(stderr, "sc_reentry: run ended at %d, not at the event\n",
                (int)stop);
        return 1;
    }

    /* The event state, the count of legs it took, and the reason the run
     * ended - the last one because it is cheap and because a future reader
     * comparing two hashes wants to know it was the same reason. */
    hash_state(&h, &vessel);
    core_hash_f64(&h, (double)legs);
    core_hash_f64(&h, (double)stop);

    /* And the altitude it actually stopped at, which is the number the whole
     * step is about. Hashed rather than checked here: the check with a
     * tolerance lives in core/test/test_prop.c, and this says that whatever
     * the root finder produced, it produced the same bits everywhere. */
    State earth_at;
    if (eph_body_state(eph, EARTH, vessel.t, &earth_at) != CORE_OK) {
        return 1;
    }
    core_hash_f64(&h, vec3_distance(vessel.r, earth_at.r) - surface);

    prop_free(p);
    eph_free(eph);

    printf("sc_reentry %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
