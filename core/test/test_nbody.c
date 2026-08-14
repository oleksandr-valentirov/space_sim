/* Mutual N-body against JPL Horizons (ROADMAP B5).
 *
 * The purpose of this comparison is to catch mistakes, not to reach JPL's
 * accuracy. A wrong frame, centre or unit shows up as hundreds of thousands
 * of kilometres or more; missing physics shows up as far less. The test is
 * built to tell those two apart, because only the first is a bug.
 *
 * Run from the repository root. */

#include "nbody.h"
#include "refdata.h"
#include "test.h"

#include <math.h>
#include <string.h>

#define MAX_SAMPLES 256
#define AU_M 1.495978707e11

/* Tight enough that the slowest-converging body in the set - the Moon, always
 * the Moon - has stopped moving. Same number the ephemeris cooker uses, and
 * for the same reason; the tolerance block at the end of main() is where it
 * was measured and where the cost of being looser is written down. */
#define CONVERGED_TOL_M 1.0e-6

/* Every major body of the solar system, in the order the fixtures use. */
static const char *ALL_BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

/* The Milestone 0 system, kept as a deliberate contrast. */
static const char *MINIMAL_BODIES[] = { "sun", "earth", "moon" };
#define N_MINIMAL (sizeof MINIMAL_BODIES / sizeof MINIMAL_BODIES[0])

static RefSample reference[N_ALL][MAX_SAMPLES];
static size_t n_samples;
static RefGm gm_table[16];
static size_t n_gm;

typedef struct {
    double max_earth;            /* against Horizons, in the SSB frame */
    double max_earth_relative;   /* with the model's own barycentre removed */
    double max_moon_geocentric;  /* Moon relative to Earth: the geometry the
                                  * game is actually built on */
    double energy_drift;
    double barycentre_drift;
    long   steps;
} RunResult;

static int index_of(const char *name)
{
    for (size_t i = 0; i < N_ALL; i++) {
        if (strcmp(ALL_BODIES[i], name) == 0) {
            return (int)i;
        }
    }
    return -1;
}

static int load_fixtures(void)
{
    char path[128];

    for (size_t i = 0; i < N_ALL; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n) != CORE_OK) {
            fprintf(stderr, "  cannot load %s\n", path);
            return 0;
        }
        if (i == 0) {
            n_samples = n;
        } else if (n != n_samples) {
            fprintf(stderr, "  %s has %zu samples, expected %zu\n",
                    path, n, n_samples);
            return 0;
        }
    }

    return refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm) == CORE_OK;
}

/* Integrates the named bodies from J2000 across the whole fixture, stopping
 * at every reference epoch to measure the divergence. */
static RunResult run_model(const char **names, size_t n, double tol_m)
{
    RunResult out;
    memset(&out, 0, sizeof out);

    NBodySystem sys;
    memset(&sys, 0, sizeof sys);
    sys.n = n;

    int map[NBODY_MAX];
    int earth = -1, moon = -1;
    State current[NBODY_MAX];

    for (size_t i = 0; i < n; i++) {
        map[i] = index_of(names[i]);
        CHECK(map[i] >= 0);
        sys.mu[i] = refdata_gm_of(gm_table, n_gm, names[i]);
        CHECK(sys.mu[i] > 0.0);
        current[i] = reference[map[i]][0].s;

        if (strcmp(names[i], "earth") == 0) {
            earth = (int)i;
        }
        if (strcmp(names[i], "moon") == 0) {
            moon = (int)i;
        }
    }
    CHECK(earth >= 0 && moon >= 0);

    double energy0 = nbody_energy(&sys, current);
    Vec3d barycentre0 = nbody_barycentre(&sys, current);

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol_m;
    cfg.max_steps = 5000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    for (size_t s = 1; s < n_samples; s++) {
        State next[NBODY_MAX];
        CoreResult r = nbody_integrate(&sys, current, reference[0][s].s.t,
                                       &cfg, &st, next);
        CHECK(r == CORE_OK);
        if (r != CORE_OK) {
            return out;
        }
        memcpy(current, next, sizeof next);

        /* The same barycentre computed from the model and from the reference,
         * over the same bodies with the same weights. Subtracting it removes
         * the bulk translation of the subsystem and leaves the part of the
         * error that is a genuine distortion of the orbits. */
        State ref_now[NBODY_MAX];
        for (size_t i = 0; i < n; i++) {
            ref_now[i] = reference[map[i]][s].s;
        }
        Vec3d bary_model = nbody_barycentre(&sys, current);
        Vec3d bary_ref = nbody_barycentre(&sys, ref_now);

        double d_earth = vec3_distance(current[earth].r, ref_now[earth].r);
        double d_earth_rel = vec3_distance(
            vec3_sub(current[earth].r, bary_model),
            vec3_sub(ref_now[earth].r, bary_ref));
        double d_moon_geo = vec3_distance(
            vec3_sub(current[moon].r, current[earth].r),
            vec3_sub(ref_now[moon].r, ref_now[earth].r));

        if (d_earth > out.max_earth) {
            out.max_earth = d_earth;
        }
        if (d_earth_rel > out.max_earth_relative) {
            out.max_earth_relative = d_earth_rel;
        }
        if (d_moon_geo > out.max_moon_geocentric) {
            out.max_moon_geocentric = d_moon_geo;
        }
    }

    double energy1 = nbody_energy(&sys, current);
    out.energy_drift = fabs((energy1 - energy0) / energy0);
    out.barycentre_drift = vec3_distance(nbody_barycentre(&sys, current),
                                         barycentre0);
    out.steps = st.n_accepted;
    return out;
}

int main(void)
{
    if (!load_fixtures()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }
    CHECK(n_samples == 122);

    /* The N-body integrator against the two-body one. Give the second body
     * zero mass and it becomes a test particle in the first body's field,
     * which is exactly the problem dop853_integrate solves. Two
     * implementations agreeing is the check; a shared coefficient table is
     * all they have in common. */
    {
        NBodySystem sys;
        memset(&sys, 0, sizeof sys);
        sys.n = 2;
        sys.mu[0] = 3.98600435436e14;
        sys.mu[1] = 0.0;

        double radius = 7.0e6;
        double speed = sqrt(sys.mu[0] / radius);

        State y[2];
        y[0] = (State){ { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, 0.0 };
        y[1] = (State){ { radius, 0.0, 0.0 }, { 0.0, speed, 0.0 }, 0.0 };

        double period = two_body_period(y[1].r, y[1].v, sys.mu[0]);

        Dop853Config cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-6;

        Dop853State st_n;
        memset(&st_n, 0, sizeof st_n);
        State out_n[2];
        CHECK(nbody_integrate(&sys, y, 5.0 * period, &cfg, &st_n, out_n)
              == CORE_OK);

        TwoBodyCtx ctx = { sys.mu[0] };
        Dop853State st_1;
        memset(&st_1, 0, sizeof st_1);
        State out_1;
        CHECK(dop853_integrate(accel_two_body, &ctx, &y[1], 5.0 * period,
                               &cfg, &st_1, &out_1) == CORE_OK);

        CHECK(vec3_distance(out_n[1].r, out_1.r) < 1e-3);

        /* A massless body pulls on nothing, so the attractor must not move. */
        CHECK(vec3_norm(out_n[0].r) < 1e-9);
    }

    /* Earth's oblateness (ROADMAP K2), checked as wiring rather than as new
     * physics - harmonics_accel and harmonics_potential already have their
     * own tests. What is new here is nbody_accel's reciprocal force and
     * nbody_energy's matching potential term, and both get checked against
     * an invariant that does not depend on trusting either formula: the
     * pair conserves momentum and energy, which it can only do if the
     * reaction and the potential are the derivative pair they are supposed
     * to be. */
    {
        double mu_earth = 3.98600435436e14;
        double mu_moon = 4.9028000661e12;

        HarmonicsField field = { 0 };
        field.degree = 2;
        field.re = 6378137.0;
        /* J2 (IERS 2010) from data/horizons/obj_earth.txt. */
        harmonics_set_unnormalised(&field, 2, 0, -1.08262545e-3, 0.0);

        NBodySystem sys;
        memset(&sys, 0, sizeof sys);
        sys.n = 2;
        sys.mu[0] = mu_earth;
        sys.mu[1] = mu_moon;
        sys.field[0] = &field;

        State states[2];
        states[0] = (State){ { 0.0, 0.0, 0.0 }, { 0.0, 0.0, 0.0 }, 0.0 };
        states[1] = (State){ { 3.0e8, 1.0e8, 0.5e8 }, { -30.0, 900.0, 120.0 },
                             0.0 };

        Vec3d acc_with[2];
        nbody_accel(&sys, states, acc_with);

        NBodySystem sys_off = sys;
        sys_off.field[0] = NULL;
        Vec3d acc_without[2];
        nbody_accel(&sys_off, states, acc_without);

        Vec3d d = vec3_sub(states[1].r, states[0].r);
        Vec3d a_j2;
        harmonics_accel(&field, d, mu_earth, &a_j2);

        /* Composition: the field acting on the Moon is exactly the
         * point-mass term plus harmonics_accel's own output, bit for bit -
         * nbody_accel does nothing to it in between. */
        Vec3d expect_moon = vec3_add(acc_without[1], a_j2);
        CHECK_BITS_EQ(acc_with[1].x, expect_moon.x);
        CHECK_BITS_EQ(acc_with[1].y, expect_moon.y);
        CHECK_BITS_EQ(acc_with[1].z, expect_moon.z);

        /* Reaction on Earth: the same a_j2, scaled by -mu_moon/mu_earth, the
         * same vec3_add_scaled call nbody.c itself makes - so this is bit
         * exact too, not merely close. */
        Vec3d expect_earth = vec3_add_scaled(acc_without[0], a_j2,
                                             -mu_moon / mu_earth);
        CHECK_BITS_EQ(acc_with[0].x, expect_earth.x);
        CHECK_BITS_EQ(acc_with[0].y, expect_earth.y);
        CHECK_BITS_EQ(acc_with[0].z, expect_earth.z);

        /* Momentum conservation: mu_earth*a_earth + mu_moon*a_moon from the
         * J2 term alone must vanish, to floating-point rounding rather than
         * exactly. The tolerance is looser than a round trip through
         * mu_moon/mu_earth alone would need (that is a few ULP) because
         * j2_earth is itself a subtraction of two full accelerations
         * roughly eight orders of magnitude above the reaction it isolates
         * - Earth's pull from Moon's point mass swamps Earth's pull from
         * its own J2 reaction - so the cancellation, not the ratio, sets
         * the noise floor. Measured ~9.5e-11 of scale; a wrong sign or a
         * wrong mu ratio would fail this by many orders, not by one. */
        Vec3d j2_earth = vec3_sub(acc_with[0], acc_without[0]);
        Vec3d j2_moon = vec3_sub(acc_with[1], acc_without[1]);
        Vec3d net = vec3_add(vec3_scale(j2_earth, mu_earth),
                             vec3_scale(j2_moon, mu_moon));
        double scale = mu_moon * vec3_norm(a_j2);
        CHECK(vec3_norm(net) < 1e-6 * scale);

        /* Energy conservation over an integrated arc: if the reaction were
         * missing, mistimed or the wrong magnitude, or if nbody_energy's J2
         * term did not match nbody_accel's, this would drift instead of
         * holding flat the way every other integrator check in this file
         * does. */
        double energy0 = nbody_energy(&sys, states);

        Dop853Config cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-3;
        cfg.max_steps = 100000;

        Dop853State st;
        memset(&st, 0, sizeof st);
        State next[2];
        CHECK(nbody_integrate(&sys, states, 5.0 * 86400.0, &cfg, &st, next)
              == CORE_OK);

        double energy1 = nbody_energy(&sys, next);
        CHECK(fabs((energy1 - energy0) / energy0) < 1e-9);
    }

    /* Sun, Earth and Moon alone: kept as the contrast that makes the point
     * below visible. Measured over ten years: 5.417e9 m against Horizons in
     * the SSB frame, which sounds catastrophic and is not a bug. A wrong frame
     * or centre would put it above 1e11 m, an AU or more.
     *
     * Both runs here use CONVERGED_TOL_M rather than the 1e0 they used until
     * the ephemeris span study measured what that costs - see the tolerance
     * block at the bottom of this function. The Earth's numbers are unchanged
     * by that (they were already converged at 1e0); the Moon's are not. */
    RunResult minimal = run_model(MINIMAL_BODIES, N_MINIMAL, CONVERGED_TOL_M);
    {
        CHECK(minimal.max_earth > 1e9);
        CHECK(minimal.max_earth < 2e10);

        /* Energy of the isolated system is conserved by the true dynamics, so
         * its drift measures the integrator while the position error measures
         * the model. Separating those two is the point of this whole step.
         * Measured 4.23e-11 at tol 1e1, tracking tolerance closely: 9.10e-9
         * at 1e3 and 2.99e-12 at 1e0. */
        CHECK(minimal.energy_drift < 1e-9);

        /* Nearly all of the error is the subsystem translating as a whole.
         * Measured 5.417e9 m absolute against 5.605e7 m once the model's own
         * barycentre is removed - a factor of 97.
         *
         * The cause is momentum: three bodies carry only part of the solar
         * system's, so their common barycentre drifts in a straight line
         * through the SSB frame. Measured drift 4.959e9 m, identical at every
         * tolerance from 1e5 down to 1e0 m, which is what proves it is
         * physics and not integration error. */
        CHECK(minimal.max_earth_relative < minimal.max_earth / 20.0);
        CHECK(minimal.barycentre_drift > 1e9);

        /* Geocentric lunar geometry, on the other hand, is already decent:
         * 1.843e5 m over ten years from three point masses. The Earth-Moon
         * system barely notices that the rest of the solar system is
         * missing - it notices only where it is as a whole. */
        CHECK(minimal.max_moon_geocentric < 1e6);
    }

    /* Every major body: the fix that the measurement above pointed at. The
     * missing momentum is no longer missing, and the drift collapses with it.
     *
     * Measured over ten years: Earth 1.681e6 m against Horizons, down from
     * 5.417e9 - a factor of 3223. Barycentre drift 1.051e6 m, down from
     * 4.959e9. The Moon improves too, 1.843e5 m to 1.549e5 m geocentric, but
     * only by a sixth: what the planets fix is where the subsystem is, not
     * the shape of the orbit inside it. For a model that is still nothing but
     * point masses, 1700 km over a decade at one AU is a good place to be. */
    RunResult full = run_model(ALL_BODIES, N_ALL, CONVERGED_TOL_M);
    {
        CHECK(full.max_earth < minimal.max_earth / 100.0);
        CHECK(full.barycentre_drift < minimal.barycentre_drift / 100.0);
        CHECK(full.max_earth < 1e7);
        CHECK(full.energy_drift < 1e-9);

        CHECK(full.max_moon_geocentric < minimal.max_moon_geocentric);
        CHECK(full.max_moon_geocentric < 5e5);
    }

    /* Tolerance sensitivity, and the trap it exposes.
     *
     * The Earth's error is model-limited: 1.659e6, 1.679e6, 1.681e6 m at tol
     * 1e1, 1e0, 1e-1. Converged, so no amount of integrator tuning closes
     * that gap - it closes by adding physics.
     *
     * The Moon is not, and this has now cost two wrong conclusions rather
     * than one. Geocentric error runs 8.5e8, 3.7e8, 4.0e7, 3.0e6, 9.32e4,
     * 1.417e5, 1.539e5, 1.548e5, 1.549e5 m as the tolerance tightens from 1e6
     * to 1e-6. A single global tolerance in metres treats the Moon's 3.8e8 m
     * orbit and Neptune's 4.5e12 m one alike, and the Moon is the fastest
     * thing in the system, so it converges last.
     *
     * The first wrong conclusion was reading the tol 1e1 figure as a physical
     * limit, which produced a confident and completely wrong claim about the
     * Earth's oblateness.
     *
     * The second was written here as the fix for the first: that tol 1e0 was
     * where the Moon had converged, at 9.32e4 m. It is not. It is where the
     * sequence crosses the converged value on its way past - the integration
     * error there partly cancels the missing physics, and the reading comes
     * out 40% BELOW the truth of 1.549e5 m. That is the failure mode worth
     * remembering: an unconverged number that is too small looks like
     * success, and no bound of the form "is the error below X" can catch it.
     * Only running the same thing twice at different tolerances can, which is
     * what the block below now does.
     *
     * Practical consequence for the ephemeris cooker: pick the tolerance from
     * the fastest body, not from a whole-system error norm; verify
     * convergence per body rather than in aggregate; and verify it by
     * agreement between two tolerances, not by the size of one. */
    {
        RunResult loose = run_model(ALL_BODIES, N_ALL, 1e1);

        CHECK(full.steps > loose.steps);

        double earth_change = fabs(full.max_earth - loose.max_earth)
                            / loose.max_earth;
        CHECK(earth_change < 0.05);

        /* The Moon moves by more than an order of magnitude over the same
         * tolerance change, which is what makes the point. */
        CHECK(loose.max_moon_geocentric > 10.0 * full.max_moon_geocentric);

        /* And the claim that `full` itself is converged, enforced rather than
         * asserted in a comment. Four orders of tolerance apart, agreeing to
         * better than a percent: 1.539e5 m against 1.549e5 m. Had this check
         * existed, the 9.32e4 m at tol 1e0 would have failed it by 40%. */
        RunResult check = run_model(ALL_BODIES, N_ALL, 1e-2);
        double moon_change = fabs(full.max_moon_geocentric
                                  - check.max_moon_geocentric)
                           / check.max_moon_geocentric;
        CHECK(moon_change < 0.02);

        /* The barycentre drift does not move at all with tolerance, which is
         * what identifies it as physical rather than numerical. Measured
         * 1.0509e6 m at every tolerance from 1e6 down to 1e-1. */
        double bary_change = fabs(full.barycentre_drift - loose.barycentre_drift)
                           / loose.barycentre_drift;
        CHECK(bary_change < 1e-3);
    }

    /* Anchoring the barycentre, and the claim it rests on.
     *
     * The drift above is not a mystery to be integrated away, it is momentum
     * the initial conditions carry: 3.35e-3 m/s of it for this set, which
     * over ten years is 1.057e6 m against the 1.051e6 m measured. Naming the
     * cause is what makes removing it legitimate rather than a fudge - the
     * true solar system's barycentre is at rest by construction, and ours
     * moves only because the set is incomplete.
     *
     * Checked here rather than only in the cooker because the arithmetic is
     * the claim: subtract the mass-weighted mean velocity and the system's
     * momentum is gone, whatever anyone then builds with it. */
    {
        NBodySystem sys;
        State current[NBODY_MAX];
        memset(&sys, 0, sizeof sys);
        sys.n = N_ALL;

        for (size_t i = 0; i < N_ALL; i++) {
            sys.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i]);
            CHECK(sys.mu[i] > 0.0);
            current[i] = reference[i][0].s;
        }

        Vec3d before = nbody_momentum_velocity(&sys, current);
        CHECK(vec3_norm(before) > 3.0e-3);
        CHECK(vec3_norm(before) < 4.0e-3);

        /* Predicted drift from the momentum alone, against the measured one:
         * they agree to under a percent, which is what says the drift is this
         * and not something else wearing its shape. */
        double predicted = vec3_norm(before) * 10.0 * 365.25 * 86400.0;
        CHECK(fabs(predicted - full.barycentre_drift)
              < 0.02 * full.barycentre_drift);

        nbody_anchor_barycentre(&sys, current);

        /* Not "smaller" - gone. What remains is the rounding of a sum of ten
         * terms spanning eleven orders of magnitude. */
        Vec3d after = nbody_momentum_velocity(&sys, current);
        CHECK(vec3_norm(after) < 1e-12);

        /* And it is a change of frame, not of the system: relative velocities
         * are untouched, so every orbit inside is the same orbit. */
        int earth = -1, moon = -1;
        for (size_t i = 0; i < N_ALL; i++) {
            if (strcmp(ALL_BODIES[i], "earth") == 0) {
                earth = (int)i;
            }
            if (strcmp(ALL_BODIES[i], "moon") == 0) {
                moon = (int)i;
            }
        }
        CHECK(earth >= 0 && moon >= 0);

        Vec3d rel_before = vec3_sub(reference[moon][0].s.v,
                                    reference[earth][0].s.v);
        Vec3d rel_after = vec3_sub(current[moon].v, current[earth].v);
        CHECK(vec3_distance(rel_before, rel_after) < 1e-12);
    }

    /* Sanity of the fixtures as used here, independent of any integration. */
    {
        int earth = index_of("earth");
        int moon = index_of("moon");
        CHECK(fabs(vec3_norm(reference[earth][0].s.r) / AU_M - 1.0) < 0.05);
        CHECK(vec3_distance(reference[moon][0].s.r,
                            reference[earth][0].s.r) < 4.1e8);
    }

    return TEST_RESULT();
}
