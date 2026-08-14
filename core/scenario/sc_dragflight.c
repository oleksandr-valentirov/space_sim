/* Determinism scenario: a vessel flying through air (ROADMAP K7b).
 *
 * sc_atmosphere already pins the density profile and the force in isolation.
 * This is the other half - the same math reached through the asset and the
 * propagator, which is the path the game takes - and it is the mirror of
 * sc_srpflight, for the same reasons that one exists separately from sc_prop.
 *
 * IT IS ALSO THE ONLY SCENARIO THAT READS THE DERIVATIVE OF THE ORIENTATION
 * CHANNELS. sc_orientation hashes the quaternion; the wind needs the body's
 * angular velocity, which comes from cheb_eval_deriv over those same
 * coefficients (eph_body_angular_velocity), and nothing else in the core has
 * ever asked for it. Four Chebyshev derivative evaluations per force
 * evaluation is exactly the kind of arithmetic that a compiler with a free
 * hand would reassociate.
 *
 * The orbit is low - 190 km, chosen off any band base, since the model is
 * discontinuous at those and this hash should not sit on a comparison that
 * could fall either way. It is also inclined, so that the wind is neither
 * parallel nor perpendicular to the motion and no component of the relative
 * velocity is zero.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "hash.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"

#define EARTH 3
#define DAY 86400.0

/* Above the mean radius the asset carries for the Earth, 6371010 m. */
#define ALTITUDE 190.0e3
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
        fprintf(stderr, "sc_dragflight: cannot read %s\n", ASSET);
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

    double radius = eph_body_radius(eph, EARTH) + opaque(ALTITUDE);

    /* A direction with no zero component, normalised with sqrt and division
     * only - these link without libm on purpose. */
    Vec3d dir = vec3(opaque(0.62), opaque(0.55), opaque(0.42));
    double dn = root(vec3_norm_sq(dir));
    Vec3d u = vec3_scale(dir, 1.0 / dn);

    /* Horizontal, and tilted out of the local east-west plane so that the
     * wind is skew to the motion. Due east would leave the z component of
     * the relative velocity at zero, which is the arrangement K7a found
     * hides errors. */
    Vec3d east = vec3_cross(vec3(0.0, 0.0, 1.0), u);
    east = vec3_scale(east, 1.0 / root(vec3_norm_sq(east)));
    Vec3d north = vec3_cross(u, east);
    Vec3d along = vec3_add_scaled(east, north, opaque(0.6));
    along = vec3_scale(along, 1.0 / root(vec3_norm_sq(along)));

    double speed = root(eph_body_mu(eph, EARTH) / radius);

    State vessel;
    vessel.r = vec3_add_scaled(earth.r, u, radius);
    vessel.v = vec3_add_scaled(earth.v, along, speed);
    vessel.t = t0;

    /* 20 m^2 on 1000 kg at Cd = 2.2 - a blunt body, sized so that the drag
     * is well above the tolerance rather than to model anything. Cr is zero
     * here on purpose: sunlight has its own scenario, and this hash should
     * move when the air changes and not when the shadow does. */
    VesselParams blunt;
    blunt.mass_kg = opaque(1000.0);
    blunt.area_m2 = opaque(20.0);
    blunt.cr = opaque(0.0);
    blunt.cd = opaque(2.2);

    PropConfig cfg;
    cfg.integrator = CORE_INTEG_DOP853;
    cfg.tol_m = opaque(1e-2);
    cfg.h_max_s = opaque(30.0);
    cfg.max_steps = 0;

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 1;
    }

    CoreHash h;
    core_hash_init(&h);
    hash_state(&h, &vessel);

    /* An hour is about two thirds of an orbit: enough for the vessel to
     * sweep a range of latitudes, so the wind changes both magnitude and
     * direction relative to the motion. */
    double t_end = t0 + opaque(3600.0);
    double step = 0.0;
    long legs = 0;

    for (;;) {
        State samples[CAP];
        size_t n = 0;
        State final_state;
        CoreStopReason stop;
        int event = -1;

        if (prop_run(p, &vessel, &blunt, t_end, NULL, 0, samples, CAP, &n,
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

    /* And the matrix over part of the same leg. accel_field_var now carries
     * the drag blocks - both of them, including the velocity one it ignored
     * until K7b - and nothing else in the scenarios reaches those. */
    State stm_final;
    double phi[36];
    double stm_step = 0.0;
    State restart;
    restart.r = vec3_add_scaled(earth.r, u, radius);
    restart.v = vec3_add_scaled(earth.v, along, speed);
    restart.t = t0;

    if (prop_run_stm(p, &restart, &blunt, t0 + opaque(600.0), &stm_final, phi,
                     &stm_step) != CORE_OK) {
        return 1;
    }

    hash_state(&h, &stm_final);
    core_hash_f64(&h, stm_step);
    for (int i = 0; i < 36; i++) {
        core_hash_f64(&h, phi[i]);
    }

    prop_free(p);
    eph_free(eph);

    printf("sc_dragflight %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
