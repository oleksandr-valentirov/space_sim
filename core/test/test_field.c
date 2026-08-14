/* A vessel in the ephemeris field (ROADMAP C4).
 *
 * The oracle here is worth explaining, because a force model summing point
 * masses is the kind of code that is easy to write and hard to check: it
 * agrees with any independent implementation of the same formula, including a
 * wrong one.
 *
 * So the test does not reimplement the formula. It uses the fact that the
 * cooker integrated each body under exactly this acceleration - the sum over
 * every other body - and the answer is baked into the asset. Put a massless
 * particle on a body's own state, give it the field of all the OTHER bodies,
 * and it must follow that body. If any term, sign, index or unit is wrong, it
 * will not.
 *
 * Run from the repository root. Writes into build/, which is not tracked. */

#include "eph_build.h"
#include "field.h"
#include "refdata.h"
#include "stm.h"
#include "test.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define MAX_SAMPLES 256
#define DAY 86400.0

static const char *ALL_BODIES[] = {
    "sun", "mercury", "venus", "earth", "moon",
    "mars_bary", "jupiter_bary", "saturn_bary", "uranus_bary", "neptune_bary",
};
#define N_ALL (sizeof ALL_BODIES / sizeof ALL_BODIES[0])

#define EARTH 3
#define MOON  4

#define SPAN_DAYS 200.0

static const char *PATH = "build/test_field.eph";

static RefSample reference[N_ALL][MAX_SAMPLES];
static NBodySystem system_config;
static State initial[NBODY_MAX];

/* Earth's J2, the same values core/cook/cook_fixture.c cites - so the asset
 * this test cooks for itself has the physics the shipped one has. */
static HarmonicsField earth_j2(void)
{
    HarmonicsField f;
    memset(&f, 0, sizeof f);
    f.degree = 2;
    f.re = 6378137.0;
    f.c[harmonics_index(2, 0)] = -1.08262545e-3;
    return f;
}

static int load_inputs(void)
{
    RefGm gm_table[16];
    size_t n_gm = 0;
    char path[128];

    memset(&system_config, 0, sizeof system_config);
    system_config.n = N_ALL;

    if (refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm)
        != CORE_OK) {
        return 0;
    }

    for (size_t i = 0; i < N_ALL; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", ALL_BODIES[i]);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            return 0;
        }
        system_config.mu[i] = refdata_gm_of(gm_table, n_gm, ALL_BODIES[i]);
        initial[i] = reference[i][0].s;
        if (!(system_config.mu[i] > 0.0)) {
            return 0;
        }
    }

    /* The cooker's Earth oblateness (ROADMAP K2), matching cook_fixture.c.
     * Without this the asset below would be point-mass only and the K4
     * oracle would have nothing to detect. */
    system_config.has_j2 = 1;
    system_config.j2_body = EARTH;
    system_config.j2_field = earth_j2();

    return 1;
}

static Dop853Config vessel_config(void)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = 1e-3;
    cfg.max_steps = 20000000;
    return cfg;
}

/* Largest separation between a massless particle started on body's state and
 * the body itself, sampled through the span. With point_mass_only set, the
 * asset's harmonics are dropped - the same bodies, one effect removed. */
static double tracking_error(const EphemerisCtx *eph, int body,
                             double t_begin, double t_end, int samples,
                             int point_mass_only)
{
    FieldCtx field;
    if (field_all_but(eph, body, &field) != CORE_OK) {
        return -1.0;
    }

    if (point_mass_only) {
        field_clear_harmonics(&field);
    }

    State start;
    if (eph_body_state(eph, body, t_begin, &start) != CORE_OK) {
        return -1.0;
    }

    Dop853Config cfg = vessel_config();
    Dop853State st;
    memset(&st, 0, sizeof st);

    State vessel = start;
    double worst = 0.0;

    for (int k = 1; k <= samples; k++) {
        double t = t_begin + (t_end - t_begin) * (double)k / (double)samples;

        State next;
        if (dop853_integrate(accel_field, &field, &vessel, t, &cfg, &st, &next)
            != CORE_OK) {
            return -1.0;
        }
        vessel = next;

        State truth;
        if (eph_body_state(eph, body, t, &truth) != CORE_OK) {
            return -1.0;
        }

        double d = vec3_distance(vessel.r, truth.r);
        if (d > worst) {
            worst = d;
        }
    }

    if (field.failed) {
        return -1.0;
    }
    return worst;
}

int main(void)
{
    if (!load_inputs()) {
        fprintf(stderr, "  fixtures missing; run from the repository root\n");
        return EXIT_FAILURE;
    }

    EphBuildConfig build;
    memset(&build, 0, sizeof build);
    build.t_begin = 0.0;
    build.t_end = SPAN_DAYS * DAY;
    build.interval_seconds = 8.0 * DAY;
    build.degree = 14;
    build.tol_m = 1.0;

    EphBuildReport report;
    memset(&report, 0, sizeof report);
    CHECK(eph_build(&system_config, initial, ALL_BODIES, &build, PATH, &report)
          == CORE_OK);

    EphemerisCtx *eph = NULL;
    CHECK(eph_load(PATH, &eph) == CORE_OK);
    if (eph == NULL) {
        return EXIT_FAILURE;
    }

    double t_begin, t_end;
    CHECK(eph_span(eph, &t_begin, &t_end) == CORE_OK);

    /* Body selection. */
    {
        FieldCtx all;
        CHECK(field_all_bodies(eph, &all) == CORE_OK);
        CHECK(all.n_bodies == (int)N_ALL);
        CHECK(all.failed == 0);

        FieldCtx without;
        CHECK(field_all_but(eph, EARTH, &without) == CORE_OK);
        CHECK(without.n_bodies == (int)N_ALL - 1);
        for (int i = 0; i < without.n_bodies; i++) {
            CHECK(without.body[i] != EARTH);
        }

        CHECK(field_all_bodies(NULL, &all) == CORE_ERR_INVALID_ARG);
        CHECK(field_all_bodies(eph, NULL) == CORE_ERR_INVALID_ARG);
    }

    /* The oracle. A particle on the Earth's state, in the field of everything
     * but the Earth, follows the Earth; the same for the Moon, which is the
     * harder case because it is the fastest body in the set and the one whose
     * neighbourhood is most strongly curved.
     *
     * The residual is not zero and should not be: the particle feels the
     * field of the FITTED bodies, while the asset's trajectories came from the
     * exact integration that was then fitted. So this measures the Chebyshev
     * fit error, 4.6e-2 m, propagated through 200 days.
     *
     * Measured worst separation: 8.2 m for the Earth and 0.24 m for the Moon.
     * The Earth's grows steadily - 0.41, 1.2, 2.6, 4.4, 8.2 m at forty-day
     * intervals - because a small error in a nearly Keplerian orbit turns
     * into an along-track drift that accumulates. The Moon's oscillates
     * between 0.01 and 0.24 m and does not accumulate. Both are eight orders
     * of magnitude below what any real mistake would produce: a missing body,
     * a sign, or kilometres read as metres moves a planet by a fraction of
     * its own orbit. */
    {
        double earth = tracking_error(eph, EARTH, t_begin, t_end, 50, 0);
        printf("  earth tracks to %.4g m, moon to ", earth);
        CHECK(earth >= 0.0);
        CHECK(earth < 1.0e4);

        double moon = tracking_error(eph, MOON, t_begin, t_end, 50, 0);
        printf("%.4g m (with J2)\n", moon);
        CHECK(moon >= 0.0);
        CHECK(moon < 100.0);

        CHECK(earth > 0.0);
        CHECK(moon > 0.0);

        /* The same Moon, in a field whose Earth is a point mass (ROADMAP
         * K4). This is the oracle for the harmonic term, and it is an
         * external one: the asset was cooked with Earth's J2 acting on the
         * Moon, so a vessel field without it cannot reproduce the Moon's
         * own trajectory, and the gap is the size of the physics K4 adds.
         *
         * Measured: 2942 m without the term against 0.227 m with it, a
         * factor of 13000. And 0.227 m is the fit-error baseline this test
         * measured before K2 existed at all (0.24 m), which is the second
         * half of the statement: adding the term does not merely change the
         * answer, it returns the Moon to tracking as well as it did when
         * the asset had no J2 in it to miss. A sign error would show here
         * just as loudly, which is the property worth having - this cannot
         * pass by agreeing with itself. */
        double moon_point_mass = tracking_error(eph, MOON, t_begin, t_end,
                                                50, 1);
        printf("  moon without the J2 term: %.4g m\n", moon_point_mass);
        CHECK(moon_point_mass >= 0.0);
        CHECK(moon_point_mass > 1000.0);
        CHECK(moon_point_mass > 1000.0 * moon);

        /* Earth's own oracle is no longer exact, and that is physics rather
         * than a defect (ROADMAP K4). The real Earth in the cooked asset
         * also feels the REACTION to its J2 pulling on every other body -
         * Newton's third law, core/offline/nbody.c - and a massless test
         * particle carries no reaction by construction. So this residual
         * is the reaction term, not fit error: measured 629 m against the
         * 8.2 m this test saw before K2 put J2 in the asset.
         *
         * And it cannot be fixed by any arrangement of this field: the
         * Earth is the one body this context excludes, so its harmonics go
         * out with it. There is nothing here to switch on. */
        FieldCtx around_earth;
        CHECK(field_all_but(eph, EARTH, &around_earth) == CORE_OK);
        CHECK(around_earth.n_harmonic == 0);
    }

    /* The gradient on its own, by central differences, at a point with no
     * symmetry to hide a transposed index. Measured agreement better than
     * 1e-6 relative.
     *
     * Point masses here on purpose, and only here: this block checks the
     * point-mass block of field_gradient against its own finite
     * differences, and the harmonic one is checked the same way further
     * down. Splitting them means a failure names which half is wrong. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);
        field_clear_harmonics(&field);

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* A million kilometres off the Earth, out of any plane. */
        Vec3d r = vec3(earth.r.x + 7.0e8, earth.r.y - 5.0e8, earth.r.z + 3.0e8);

        double g[9];
        field_gradient(t_begin, r, &field, g);

        const double eps = 1.0e3;

        for (int j = 0; j < 3; j++) {
            Vec3d rp = r, rm = r;
            double *pp = j == 0 ? &rp.x : (j == 1 ? &rp.y : &rp.z);
            double *pm = j == 0 ? &rm.x : (j == 1 ? &rm.y : &rm.z);
            *pp += eps;
            *pm -= eps;

            Vec3d ap, am;
            accel_field(t_begin, rp, vec3_zero(), &field, &ap);
            accel_field(t_begin, rm, vec3_zero(), &field, &am);

            double numeric[3] = {
                (ap.x - am.x) / (2.0 * eps),
                (ap.y - am.y) / (2.0 * eps),
                (ap.z - am.z) / (2.0 * eps),
            };

            for (int i = 0; i < 3; i++) {
                double scale = fabs(g[i * 3 + j]);
                CHECK(fabs(numeric[i] - g[i * 3 + j]) < 1e-5 * scale);
            }
        }

        /* Symmetric to the last bit, which is a property of how it is built
         * rather than of the arithmetic - see field_gradient. And traceless:
         * Laplace's equation holds for a sum of point-mass potentials
         * anywhere but at a source. The trace check costs nothing and fails
         * on a wrong coefficient in the outer product. */
        CHECK_BITS_EQ(g[1], g[3]);
        CHECK_BITS_EQ(g[2], g[6]);
        CHECK_BITS_EQ(g[5], g[7]);

        double trace = g[0] + g[4] + g[8];
        double scale = fabs(g[0]) + fabs(g[4]) + fabs(g[8]);
        CHECK(fabs(trace) < 1e-12 * scale);
    }

    /* The STM of a vessel trajectory, by finite differences. Same method as
     * core/test/test_stm.c, in metres this time: the perturbation is 1 km and
     * the integrator tolerance 1e-3 m, so the noise floor of the measurement
     * is around 1e-6 relative.
     *
     * Over the asset's real field, harmonics included - which is the point
     * of K8b: before it, this had to drop the Earth's shape to get an STM
     * at all. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* A vessel well clear of the Earth, moving with it. */
        State vessel = earth;
        vessel.r.z += 1.0e9;
        vessel.t = t_begin;

        double t_stop = t_begin + 20.0 * DAY;

        Dop853Config cfg = vessel_config();
        Dop853State st;
        memset(&st, 0, sizeof st);

        double phi[STM_SIZE];
        State end;
        CHECK(stm_integrate(accel_field_var, &field, &vessel, t_stop, &cfg, &st,
                            &end, phi) == CORE_OK);
        CHECK(field.failed == 0);

        const double eps_r = 1.0e3;
        const double eps_v = 1.0e-3;

        double worst = 0.0;
        double biggest = 0.0;

        for (int j = 0; j < 6; j++) {
            double eps = j < 3 ? eps_r : eps_v;

            State plus = vessel, minus = vessel;
            double *pp[6] = { &plus.r.x, &plus.r.y, &plus.r.z,
                              &plus.v.x, &plus.v.y, &plus.v.z };
            double *pm[6] = { &minus.r.x, &minus.r.y, &minus.r.z,
                              &minus.v.x, &minus.v.y, &minus.v.z };
            *pp[j] += eps;
            *pm[j] -= eps;

            State end_plus, end_minus;
            memset(&st, 0, sizeof st);
            CHECK(dop853_integrate(accel_field, &field, &plus, t_stop, &cfg,
                                   &st, &end_plus) == CORE_OK);
            memset(&st, 0, sizeof st);
            CHECK(dop853_integrate(accel_field, &field, &minus, t_stop, &cfg,
                                   &st, &end_minus) == CORE_OK);

            double p[6] = { end_plus.r.x, end_plus.r.y, end_plus.r.z,
                            end_plus.v.x, end_plus.v.y, end_plus.v.z };
            double m[6] = { end_minus.r.x, end_minus.r.y, end_minus.r.z,
                            end_minus.v.x, end_minus.v.y, end_minus.v.z };

            for (int i = 0; i < 6; i++) {
                double numeric = (p[i] - m[i]) / (2.0 * eps);
                double d = fabs(numeric - phi[i * 6 + j]);
                if (d > worst) {
                    worst = d;
                }
                if (fabs(numeric) > biggest) {
                    biggest = fabs(numeric);
                }
            }
        }

        CHECK(biggest > 1.0);
        CHECK(worst < 1e-4 * biggest);
    }

    /* The harmonic term itself, at the level of one evaluation (ROADMAP
     * K4). The oracle above proves it is right; this proves it is exactly
     * harmonics_accel and nothing else, and that a context without it is
     * untouched. */
    {
        FieldCtx plain, oblate;
        CHECK(field_all_bodies(eph, &plain) == CORE_OK);
        CHECK(field_all_bodies(eph, &oblate) == CORE_OK);

        /* oblate is what the asset says; plain is the same thing with the
         * one effect removed. */
        field_clear_harmonics(&plain);
        CHECK(plain.n_harmonic == 0);
        CHECK(oblate.n_harmonic == 1);

        /* Read back from the asset rather than restated here - and checked
         * against the constants the cooker was given, which is the claim
         * the format exists to make (ROADMAP K4b). */
        HarmonicsField j2;
        CHECK(eph_body_harmonics(eph, EARTH, &j2) == CORE_OK);
        HarmonicsField cited = earth_j2();
        CHECK(j2.degree == cited.degree);
        CHECK_BITS_EQ(j2.re, cited.re);
        CHECK_BITS_EQ(j2.c[harmonics_index(2, 0)],
                      cited.c[harmonics_index(2, 0)]);

        /* Every other body is a point mass, and says so. */
        for (size_t b = 0; b < N_ALL; b++) {
            if ((int)b == EARTH) {
                continue;
            }
            HarmonicsField h;
            CHECK(eph_body_harmonics(eph, (int)b, &h) == CORE_OK);
            CHECK(h.degree == 0);
        }

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* Low Earth orbit altitude, where J2 is a large perturbation
         * rather than the whisper it is at the Moon. */
        Vec3d r = vec3(earth.r.x + 6.9e6, earth.r.y + 1.1e6, earth.r.z + 2.3e6);

        Vec3d a_plain, a_oblate;
        accel_field(t_begin, r, vec3_zero(), &plain, &a_plain);
        accel_field(t_begin, r, vec3_zero(), &oblate, &a_oblate);

        Vec3d expected;
        harmonics_accel(&j2, vec3_sub(r, earth.r), eph_body_mu(eph, EARTH),
                        &expected);

        /* Bit-exact against the operation field.c actually performs - the
         * point-mass sum plus this vector, added once at the end.
         *
         * Written as an addition rather than as
         * (a_oblate - a_plain) == expected, which looks like the more
         * direct statement and is not checkable: the harmonic term is
         * about 1e-3 of the total, so forming the sum discards its low
         * bits, and subtracting cannot put them back. That version fails
         * by ~2e-13 relative, which is the floating point of the check
         * rather than of the code. */
        Vec3d recomposed = vec3_add(a_plain, expected);
        CHECK_BITS_EQ(a_oblate.x, recomposed.x);
        CHECK_BITS_EQ(a_oblate.y, recomposed.y);
        CHECK_BITS_EQ(a_oblate.z, recomposed.z);

        /* And it is worth having at this altitude: about 1e-3 of the total,
         * which is the ratio that makes sun-synchronous orbits exist. */
        double ratio = vec3_norm(expected) / vec3_norm(a_plain);
        CHECK(ratio > 1e-4 && ratio < 1e-2);

        CHECK(plain.failed == 0 && oblate.failed == 0);
    }

    /* An STM over a harmonic field, which K4 refused and K8b can answer.
     *
     * The check that matters is not that it returns something: it is that
     * the gradient is the derivative of the acceleration the vessel is
     * actually flying under, harmonics and all. Central differences of
     * accel_field over the real asset field say so. */
    {
        FieldCtx oblate;
        CHECK(field_all_bodies(eph, &oblate) == CORE_OK);
        CHECK(oblate.n_harmonic == 1);

        State earth;
        CHECK(eph_body_state(eph, EARTH, t_begin, &earth) == CORE_OK);

        /* Close enough to the Earth that its shape is a real part of the
         * gradient rather than a rounding detail. */
        Vec3d r = vec3(earth.r.x + 7.1e6, earth.r.y + 1.3e6,
                       earth.r.z + 2.7e6);

        double g[9];
        field_gradient(t_begin, r, &oblate, g);
        CHECK(oblate.failed == 0);

        const double eps = 20.0;
        for (int j = 0; j < 3; j++) {
            Vec3d rp = r, rm = r;
            double *pp = j == 0 ? &rp.x : (j == 1 ? &rp.y : &rp.z);
            double *pm = j == 0 ? &rm.x : (j == 1 ? &rm.y : &rm.z);
            *pp += eps;
            *pm -= eps;

            Vec3d ap, am;
            accel_field(t_begin, rp, vec3_zero(), &oblate, &ap);
            accel_field(t_begin, rm, vec3_zero(), &oblate, &am);

            double numeric[3] = {
                (ap.x - am.x) / (2.0 * eps),
                (ap.y - am.y) / (2.0 * eps),
                (ap.z - am.z) / (2.0 * eps),
            };

            for (int i = 0; i < 3; i++) {
                double want = g[i * 3 + j];
                CHECK(fabs(numeric[i] - want) < 1e-5 * fabs(want));
            }
        }

        /* Symmetric to the bit, still - the harmonic block adds only the
         * upper triangle, like the point-mass one. */
        CHECK_BITS_EQ(g[1], g[3]);
        CHECK_BITS_EQ(g[2], g[6]);
        CHECK_BITS_EQ(g[5], g[7]);

        /* And the harmonic part is a real contribution at this altitude,
         * not a term that rounds away: compare against the same gradient
         * with the shape removed. */
        FieldCtx plain;
        CHECK(field_all_bodies(eph, &plain) == CORE_OK);
        field_clear_harmonics(&plain);

        double gp[9];
        field_gradient(t_begin, r, &plain, gp);

        double diff = 0.0, scale = 0.0;
        for (int k = 0; k < 9; k++) {
            diff += fabs(g[k] - gp[k]);
            scale += fabs(gp[k]);
        }
        CHECK(diff > 1e-4 * scale);

        /* accel_field_var no longer refuses, and its perturbation blocks
         * are that same gradient applied - so an STM built on it describes
         * the field the vessel flies in. */
        Vec3d in_r[2] = { r, vec3(1.0e3, 0.0, 0.0) };
        Vec3d in_v[2] = { vec3_zero(), vec3_zero() };
        Vec3d out[2];
        accel_field_var(t_begin, in_r, in_v, 2, &oblate, out);
        CHECK(oblate.failed == 0);
        CHECK(vec3_norm(out[0]) > 0.0);
        CHECK(fabs(out[1].x - (g[0] * 1.0e3)) < 1e-12 * fabs(g[0] * 1.0e3));
    }

    /* Running off the end of the ephemeris sets the flag rather than
     * returning a plausible zero. */
    {
        FieldCtx field;
        CHECK(field_all_bodies(eph, &field) == CORE_OK);

        Vec3d a;
        accel_field(t_end + DAY, vec3(1.0e9, 0.0, 0.0), vec3_zero(), &field,
                    &a);
        CHECK(field.failed == 1);
        CHECK(vec3_norm(a) == 0.0);

        /* Sticky: a later good evaluation does not clear it. */
        accel_field(t_begin, vec3(1.0e9, 0.0, 0.0), vec3_zero(), &field, &a);
        CHECK(field.failed == 1);
        CHECK(vec3_norm(a) > 0.0);
    }

    eph_free(eph);
    remove(PATH);

    return TEST_RESULT();
}
