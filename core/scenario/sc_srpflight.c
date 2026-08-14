/* Determinism scenario: a vessel flying under radiation pressure (K6b).
 *
 * sc_srp already pins the geometry and the polynomial in isolation. This is
 * the other half - the same math reached through the asset and the
 * propagator, which is the path the game takes.
 *
 * A separate scenario rather than another block inside sc_prop, and that is
 * deliberate. sc_prop's hash had to stay exactly where it was across the
 * version 3 format bump, because "the new fields moved no coefficient" is a
 * claim core/ephemeris.h makes and this is what checks it. Folding SRP in
 * there would have moved the hash for a second reason and made the first one
 * unprovable.
 *
 * The orbit is chosen so the vessel passes through the Earth's shadow rather
 * than staying in sunlight: without an eclipse this would hash the smooth
 * 1/d^2 term alone and srp_shadow's branches would never be reached. A low
 * orbit in the plane of the Sun does that twice an hour.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define SUN   0
#define EARTH 3
#define DAY 86400.0

#define ORBIT_R 7000.0e3
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

static double root(double x)
{
    return sqrt(x);
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "sc_srpflight: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` rebuilds it\n");
        return 1;
    }

    double t_begin, t_span_end;
    if (eph_span(eph, &t_begin, &t_span_end) != CORE_OK) {
        return 1;
    }

    double t0 = t_begin + opaque(1.0 * DAY);

    State earth, sun;
    if (eph_body_state(eph, EARTH, t0, &earth) != CORE_OK ||
        eph_body_state(eph, SUN, t0, &sun) != CORE_OK) {
        return 1;
    }

    /* The orbit lies along the Earth-Sun line, so the vessel starts at local
     * noon and reaches the shadow a quarter of an orbit later. Built with
     * sqrt and divisions only, like every other scenario: these link without
     * libm on purpose. */
    Vec3d to_sun = vec3_sub(sun.r, earth.r);
    double d_sun = root(vec3_norm_sq(to_sun));
    Vec3d u = vec3_scale(to_sun, 1.0 / d_sun);

    /* Any direction perpendicular to u will do for the velocity; the cross
     * product with the frame's z axis is one, and it is exact arithmetic. */
    Vec3d w = vec3_cross(u, vec3(0.0, 0.0, 1.0));
    double wn = root(vec3_norm_sq(w));
    w = vec3_scale(w, 1.0 / wn);

    double speed = root(eph_body_mu(eph, EARTH) / opaque(ORBIT_R));

    State vessel;
    vessel.r = vec3_add_scaled(earth.r, u, opaque(ORBIT_R));
    vessel.v = vec3_add_scaled(earth.v, w, speed);
    vessel.t = t0;

    /* 20 m^2 on 1000 kg at Cr = 1.3 - a large solar panel on a small
     * spacecraft, chosen so the effect is well above the tolerance rather
     * than to model anything in particular. */
    VesselParams sail;
    sail.mass_kg = opaque(1000.0);
    sail.area_m2 = opaque(20.0);
    sail.cr = opaque(1.3);

    /* Zero, and set explicitly. This scenario builds VesselParams field by
     * field, and -Wmissing-field-initializers does not see that form - so
     * when K7b added cd, this struct silently grew an uninitialised member
     * that field_set_vessel reads as "does this vessel feel drag". Whatever
     * was on the stack would have decided, differently on each platform,
     * which is the one failure mode a determinism scenario must not have. */
    sail.cd = opaque(0.0);

    /* Braced, not field by field: -Wmissing-field-initializers only
     * sees this form, and PropConfig has grown a field twice now. */
    PropConfig cfg = { CORE_INTEG_DOP853, opaque(1e-2), opaque(60.0), 0,
                       opaque(1.0) };

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);
    hash_state(&h, &vessel);

    /* Six hours is four orbits, so four entries into shadow and four exits -
     * every branch of srp_shadow, in both directions. */
    double t_end = t0 + opaque(6.0 * 3600.0);
    double step = 0.0;
    long legs = 0;

    for (;;) {
        State samples[CAP];
        size_t n = 0;
        State final_state;
        CoreStopReason stop;
        int event = -1;

        if (prop_run(p, &vessel, &sail, t_end, NULL, 0, samples, CAP, &n,
                     &final_state, &stop, &event, &step) != CORE_OK) {
            return 1;
        }

        for (size_t i = 0; i < n; i++) {
            hash_state(&h, &samples[i]);
        }
        core_hash_f64(&h, (double)n);
        core_hash_f64(&h, step);

        vessel = final_state;
        legs++;

        if (stop == CORE_STOP_T_END) {
            break;
        }
        if (legs > 100000) {
            return 1;
        }
    }

    hash_state(&h, &vessel);
    core_hash_f64(&h, (double)legs);

    /* And the matrix over the same leg, since accel_field_var carries the
     * SRP block too and nothing else in the scenarios reaches it. */
    State stm_final;
    double phi[36];
    double stm_step = 0.0;
    State restart;
    restart.r = vec3_add_scaled(earth.r, u, opaque(ORBIT_R));
    restart.v = vec3_add_scaled(earth.v, w, speed);
    restart.t = t0;

    if (prop_run_stm(p, &restart, &sail, t0 + opaque(3600.0), &stm_final, phi,
                     &stm_step) != CORE_OK) {
        return 1;
    }

    hash_state(&h, &stm_final);
    for (int i = 0; i < 36; i++) {
        core_hash_f64(&h, phi[i]);
    }

    prop_free(p);
    eph_free(eph);

    printf("sc_srpflight %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
