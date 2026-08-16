/* Oracle for checking the FFI declarations (ROADMAP D2).
 *
 * Prints what `eph_body_state` returns in C so `tests/ffi.rs` can compare it
 * against what the same function returns through Rust. The comparison is
 * bitwise.
 *
 * Why a separate program rather than numbers written into the test: a
 * boundary error does not fail, it gives plausible numbers. Swapped `State`
 * fields, `int` instead of `size_t`, a forgotten `const` in a signature -- all
 * of it compiles and returns something. Literals baked into the test would
 * catch such an error but would go stale at the first `make cook` with no way
 * to update; here the oracle is rebuilt alongside the asset and the comparison
 * stays alive.
 *
 * Not part of the core and not a determinism scenario: in `core/scenario/` it
 * would change `golden.txt`; here it is merely crate scaffolding.
 *
 * Printed as %.17g: seventeen significant digits recover a double uniquely,
 * so the text in the middle loses nothing.
 *
 * Format: first field is a tag, then numbers in %.17g.
 *
 *   eph  <body> <t> <x> <y> <z> <vx> <vy> <vz>
 *   rad  <body> <metres>                         mean body radius
 *   mu   <body> <m^3/s^2>                        gravitational parameter
 *   samp <k> <t> <x> <y> <z> <vx> <vy> <vz>      run sample
 *   run  <count> <stop> <event> <step>           run summary
 *   end  <t> <x> <y> <z> <vx> <vy> <vz>          final state of the run
 *   cmu  <gm1> <gm2> <mu>                        pair mass fraction (CR3BP)
 *   jac  <x> <z> <vy> <mu> <C>                   Jacobi constant
 *   lag  <point> <x> <y> <z>                     Lagrange point
 *   zvc  <c> <result> <r>                        ray to the zero-velocity
 *                                                curve
 *   syn  <t> <L> <dL/dt> <rate> <mu>             synodic frame of the pair
 *   fri  <t> <x> <y> <z> <vx> <vy> <vz>          Moon in its own frame
 *
 * Two runs: one without events to a given time, one with periapsis armed. The
 * second matters on its own -- it goes through `CoreEvent`, and a struct of
 * enum, int and double in a row is exactly where layout and alignment diverge
 * quietly.
 *
 * The vessel is given as literals rather than computed: there is no `sqrt`
 * here, because the oracle links without `libm`, like the determinism
 * scenarios (build.rs).
 *
 * Run from the repository root. */

#include "cr3bp.h"
#include "ephemeris.h"
#include "frame.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"
#define DAY 86400.0

/* Indices in cooker order (core/cook/cook_fixture.c) and instants inside the
 * fixture's 120-day span. Sun and Moon on purpose: the first barely moves at
 * this scale, the second moves fastest, so a field layout error shows on one
 * of them for certain. */
static const int BODIES[] = { 0, 3, 4 };
#define N_BODIES (sizeof BODIES / sizeof BODIES[0])

static const double TIMES[] = { 0.0, 30.0 * DAY, 119.0 * DAY };
#define N_TIMES (sizeof TIMES / sizeof TIMES[0])

/* An index past the end of any asset we cook (ROADMAP U2a). */
#define NO_SUCH_BODY 99

/* A vessel on an elongated Earth orbit: offset from Earth and velocity given
 * as numbers. 0.8 of circular speed at geostationary radius, i.e. an orbit
 * whose periapsis is worth searching for. */
#define VESSEL_T0 (1.0 * DAY)
#define VESSEL_DX 42164.0e3
#define VESSEL_VY 1967.84
#define VESSEL_VZ 1475.88

/* And a second vessel, low (ROADMAP K7b). The one above hangs at 35786 km,
 * where there is no air at all -- a run with non-zero `cd` there would print
 * what it prints without it, and swapped `cr` and `cd` would pass the
 * comparison.
 *
 * 320 km above the equatorial radius, near-circular, inclined: the velocity is
 * literals like everything else here, because the oracle links without
 * libm. */
#define LEO_DX 6698137.0
#define LEO_VY 6680.0
#define LEO_VZ 3860.0

#define CAP 64

static void print_state(const char *tag, const State *s)
{
    printf("%s %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
           tag, s->t, s->r.x, s->r.y, s->r.z, s->v.x, s->v.y, s->v.z);
}

static int propagate(const EphemerisCtx *eph)
{
    State earth;
    if (eph_body_state(eph, 3, VESSEL_T0, &earth) != CORE_OK) {
        return 0;
    }

    State vessel;
    vessel.r = vec3(earth.r.x + VESSEL_DX, earth.r.y, earth.r.z);
    vessel.v = vec3(earth.v.x, earth.v.y + VESSEL_VY, earth.v.z + VESSEL_VZ);
    vessel.t = VESSEL_T0;

    /* Braced, so that the next field PropConfig grows is a compile error here
     * rather than whatever the stack held (K7b). */
    PropConfig cfg = { CORE_INTEG_DOP853, 1e-2, 1800.0, 0, 1.0 };

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 0;
    }

    State samples[CAP];
    size_t n = 0;
    State final_state;
    CoreStopReason stop;
    int event = -1;
    double step = 0.0;

    if (prop_run(p, &vessel, NULL, VESSEL_T0 + 0.5 * DAY, NULL, 0, samples, CAP, &n,
                 &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    for (size_t k = 0; k < n; k++) {
        printf("samp %zu %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
               k, samples[k].t, samples[k].r.x, samples[k].r.y, samples[k].r.z,
               samples[k].v.x, samples[k].v.y, samples[k].v.z);
    }
    printf("run %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("end", &final_state);

    /* The same vessel, but an event stops the run. */
    CoreEvent ev;
    ev.kind = CORE_EVENT_PERIAPSIS;
    ev.body_id = 3;
    ev.param = 0.0;

    step = 0.0;
    if (prop_run(p, &vessel, NULL, VESSEL_T0 + 4.0 * DAY, &ev, 1, NULL, 0, &n,
                 &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("run %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("end", &final_state);

    /* The same leg, but with the transition matrix (ROADMAP K8). Both the
     * final state and the step are printed: the boundary promises this is
     * bit-identical to what prop_run would give, so the comparison must see
     * both. */
    step = 0.0;
    double phi[36];
    if (prop_run_stm(p, &vessel, NULL, VESSEL_T0 + 0.5 * DAY, &final_state, phi,
                     &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("stmrun %.17g\n", step);
    print_state("stmend", &final_state);
    for (int i = 0; i < 36; i++) {
        printf("stm %d %.17g\n", i, phi[i]);
    }

    /* And the same leg with a vessel that feels radiation pressure (K6b).
     * Every boundary argument must be non-null here at least once: `vessel` as
     * NULL is already printed above, and a pointer nobody dereferences would
     * not prove the struct fields are declared in the same order. */
    VesselParams sail;
    sail.mass_kg = 1000.0;
    sail.area_m2 = 20.0;
    sail.cr = 1.3;
    sail.cd = 0.0;

    step = 0.0;
    if (prop_run(p, &vessel, &sail, VESSEL_T0 + 0.5 * DAY, NULL, 0, samples,
                 CAP, &n, &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("srprun %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("srpend", &final_state);

    /* And a low orbit with a vessel that feels air (ROADMAP K7b). Ten
     * minutes: at 320 km that is enough for drag to move the last bits well
     * past the comparison threshold, and little enough for the leg to stay one
     * leg. */
    State low;
    low.r = vec3(earth.r.x + LEO_DX, earth.r.y, earth.r.z);
    low.v = vec3(earth.v.x, earth.v.y + LEO_VY, earth.v.z + LEO_VZ);
    low.t = VESSEL_T0;

    VesselParams blunt;
    blunt.mass_kg = 1000.0;
    blunt.area_m2 = 20.0;
    blunt.cr = 1.3;
    blunt.cd = 2.2;

    step = 0.0;
    if (prop_run(p, &low, &blunt, VESSEL_T0 + 600.0, NULL, 0, samples,
                 CAP, &n, &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("dragrun %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("dragend", &final_state);

    prop_free(p);
    return 1;
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "oracle: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root\n");
        return 1;
    }

    for (size_t b = 0; b < N_BODIES; b++) {
        for (size_t k = 0; k < N_TIMES; k++) {
            State s;
            if (eph_body_state(eph, BODIES[b], TIMES[k], &s) != CORE_OK) {
                fprintf(stderr, "oracle: body %d at t = %g failed\n",
                        BODIES[b], TIMES[k]);
                eph_free(eph);
                return 1;
            }

            printf("eph %d %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
                   BODIES[b], TIMES[k],
                   s.r.x, s.r.y, s.r.z, s.v.x, s.v.y, s.v.z);
        }

        printf("rad %d %.17g\n", BODIES[b], eph_body_radius(eph, BODIES[b]));
        printf("mu %d %.17g\n", BODIES[b], eph_body_mu(eph, BODIES[b]));

        /* Orientation, and all four components printed separately on purpose
         * (ROADMAP-PLANETS.md R1c). Half the world writes a quaternion as
         * (x, y, z, w) and the other half as (w, x, y, z); a declaration that
         * picked the wrong one would still be a valid rotation, just not this
         * one, and the only place it would show is a planet facing the wrong
         * way. Two of the fixture's bodies carry rotation channels and eight
         * do not - the latter answer with the identity, which is also worth
         * pinning: "not modelled" must not drift into "failed". */
        for (size_t k = 0; k < N_TIMES; k++) {
            Quat q;
            if (eph_body_orientation(eph, BODIES[b], TIMES[k], &q) != CORE_OK) {
                fprintf(stderr, "oracle: orientation of %d at t = %g failed\n",
                        BODIES[b], TIMES[k]);
                eph_free(eph);
                return 1;
            }
            printf("quat %d %.17g %.17g %.17g %.17g %.17g\n",
                   BODIES[b], TIMES[k], q.w, q.x, q.y, q.z);
        }
    }

    /* And a body the asset has never heard of (ROADMAP U2a). The zero it
     * returns is the same zero as "the asset does not say how big it is", and
     * that is the whole contract: a caller who never checks a result code
     * still cannot be handed a size that was invented for it. A declaration
     * that got the argument type wrong - int where C expects int, but the
     * other way around on some ABI - would show up right here, because an
     * in-range index would keep answering plausibly. */
    printf("rad %d %.17g\n", NO_SUCH_BODY,
           eph_body_radius(eph, NO_SUCH_BODY));

    /* CR3BP: pair mass fraction, Jacobi constant, Lagrange points and the ray
     * to the zero-velocity curve (ROADMAP-UI.md, U6b2).
     *
     * The numbers are dimensionless, which is exactly why they are here: a
     * declaration that swapped the argument order of `cr3bp_jacobi(r, v, mu)`
     * would return a perfectly plausible constant -- just not the right one.
     * The state comes from the JPL catalogue (halo 1151) rather than being
     * invented: `core/test/test_correct.c` names it x = 1.169,
     * vy = -0.194. */
    {
        double mu = cr3bp_mu(eph_body_mu(eph, 3), eph_body_mu(eph, 4));
        printf("cmu %.17g %.17g %.17g\n", eph_body_mu(eph, 3),
               eph_body_mu(eph, 4), mu);

        static const double HALO[3] = { 1.1690, -0.0980, -0.1940 };
        Vec3d r = vec3(HALO[0], 0.0, HALO[1]);
        Vec3d v = vec3(0.0, HALO[2], 0.0);
        printf("jac %.17g %.17g %.17g %.17g %.17g\n", HALO[0], HALO[1],
               HALO[2], mu, cr3bp_jacobi(r, v, mu));

        for (int point = 1; point <= 5; point++) {
            Vec3d l;
            if (cr3bp_lagrange(mu, point, &l) != CORE_OK) {
                fprintf(stderr, "oracle: lagrange %d failed\n", point);
                eph_free(eph);
                return 1;
            }
            printf("lag %d %.17g %.17g %.17g\n", point, l.x, l.y, l.z);
        }

        /* The gate near L1: slightly above C(L1) there is a crossing,
         * slightly below there is no answer for the ray at all -- and that is
         * an answer, not a failure. */
        Vec3d l1;
        cr3bp_lagrange(mu, 1, &l1);
        double c1 = cr3bp_jacobi(l1, vec3_zero(), mu);
        const double DELTA[2] = { 0.01, -0.01 };
        for (size_t k = 0; k < 2; k++) {
            double r_out = 0.0;
            CoreResult result = cr3bp_zvc_radius(mu, c1 + DELTA[k], vec3_zero(),
                                                 vec3(1.0, 0.0, 0.0), 0.95,
                                                 &r_out);
            printf("zvc %.17g %d %.17g\n", c1 + DELTA[k], (int)result, r_out);
        }
    }

    /* The synodic frame and the transform into it (ROADMAP-UI.md, U6b1).
     *
     * What is printed is not the basis itself but the quantities defining it,
     * plus the Moon's state taken into its own frame: there, by construction,
     * it sits at (1 - mu, 0, 0) almost motionless, and any SynodicFrame layout
     * error would spoil exactly that. C fills the struct itself, so a size
     * mismatch would mean a write past the end, not a strange number. */
    for (size_t k = 0; k < N_TIMES; k++) {
        SynodicFrame f;
        if (frame_synodic(eph, 3, 4, TIMES[k], &f) != CORE_OK) {
            fprintf(stderr, "oracle: synodic frame at t = %g failed\n", TIMES[k]);
            eph_free(eph);
            return 1;
        }
        printf("syn %.17g %.17g %.17g %.17g %.17g\n", TIMES[k], f.length,
               f.length_rate, f.rate, f.mu);

        State moon, moon_syn;
        if (eph_body_state(eph, 4, TIMES[k], &moon) != CORE_OK) {
            fprintf(stderr, "oracle: moon at t = %g failed\n", TIMES[k]);
            eph_free(eph);
            return 1;
        }
        frame_from_inertial(&f, &moon, &moon_syn);
        printf("fri %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n", TIMES[k],
               moon_syn.r.x, moon_syn.r.y, moon_syn.r.z,
               moon_syn.v.x, moon_syn.v.y, moon_syn.v.z);
    }

    if (!propagate(eph)) {
        eph_free(eph);
        return 1;
    }

    eph_free(eph);
    return 0;
}
