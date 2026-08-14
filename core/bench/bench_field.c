/* Force-model cost benchmark: one accel_field evaluation (ROADMAP L1, debt
 * D6, skill perf-probe).
 *
 * The sibling bench_dop853.c measures the integrator on accel_two_body, and
 * that number has carried a warning since K6a: it is not what the game pays
 * for. A vessel in the game flies in accel_field - ten Chebyshev evaluations
 * of the ephemeris, harmonics for the Earth, radiation pressure with a
 * conical shadow, drag through a co-rotating atmosphere - and K6a measured
 * one such evaluation at 2.1 us against 0.641 us for a whole accepted step
 * of the two-body benchmark. A benchmark whose number is three times smaller
 * than one call of the real force model is not measuring the same thing.
 *
 * So this is a second benchmark, not a replacement (skill perf-probe says to
 * widen the table rather than grow a second skill). Two numbers answer two
 * questions: what an integrator step costs, and what one force evaluation in
 * the model the game actually flies costs.
 *
 * WHAT IT PRINTS, AND WHY A BREAKDOWN. Four cumulative configurations - point
 * masses, plus harmonics, plus SRP, plus drag - each differing from the one
 * above it by exactly one term. K7b claimed drag costs 41%, and until now
 * that claim had nothing to check it against; here it is a line of output.
 * The configurations differ only in the vessel and the harmonics, never in
 * the states, so the deltas are the terms and nothing else.
 *
 * WHY A PRE-COMPUTED ARC. Timing one fixed state would measure the ephemeris
 * with a hot Chebyshev interval and a hot cache, which is the best case
 * rather than the honest one. Timing states generated inside the loop would
 * measure the generator. So the arc is integrated first - by the same
 * accel_field, which is not circular because it is outside the timed region -
 * and the loop then walks the stored states.
 *
 * Same rules as the other benchmark: wall-clock time is hardware-dependent by
 * definition, so this prints numbers rather than a hash and is never compared
 * against core/scenario/golden.txt. Links against libcore.a only, without
 * -lm: if a stray libm call reached the force model, linking would fail here
 * before any number was measured. */

/* clock_gettime/CLOCK_MONOTONIC are POSIX, not C11 - see bench_dop853.c. */
#define _POSIX_C_SOURCE 199309L

#include "accel.h"
#include "ephemeris.h"
#include "field.h"
#include "harmonics.h"
#include "integrator.h"

#include <stdio.h>
#include <string.h>
#include <time.h>

/* The committed fixture, ten bodies, cooked once by core/cook/cook_fixture.c.
 * Cooking one here would make the measurement depend on this machine twice
 * over: the Chebyshev fit calls cos(), so two machines would time two
 * different assets. */
#define ASSET "data/fixture/earth_moon.eph"
#define EARTH 3
#define MOON  4

/* A low orbit, because that is where every term of the model is live at once:
 * harmonics matter, the shadow is crossed, and the air is thick enough to
 * measure. Higher up, drag would still be computed and would still cost what
 * it costs, but the state would be a less honest example of the model.
 *
 * 300 km above the Earth's mean radius, and the speed is written out rather
 * than derived as sqrt(mu/r): this file links without -lm on purpose, and a
 * benchmark has no business being the one place that tests that rule.
 * Inclined, because a state whose z components are all zero was exactly the
 * trap K7b fell into - there the drag Jacobian's cells came out of noise. */
#define ALTITUDE_M 3.0e5
static const Vec3d OFFSET_R = { 6.671010e6, 0.0, 0.0 };
static const Vec3d OFFSET_V = { 0.0, 7.100e3, 3.050e3 };

/* Enough states to cross a few revolutions of a 90-minute orbit, and small
 * enough to stay in cache - the point is to vary the ephemeris argument, not
 * to measure memory. */
#define SAMPLES 1024
#define SAMPLE_DT_S 30.0

/* Each configuration gets its own wall-clock budget. Half a second at a
 * microsecond a call is half a million samples, which is far more than
 * needed to separate terms that differ by tens of per cent. */
#define BUDGET_S 0.5

/* The degree K5 is aiming the Moon at (PROJECT.md section 4: GRAIL, trimmed
 * near 50). Nothing computes at that degree yet - see harmonics_scaling. */
#define K5_DEGREE 50

static double now_s(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

/* One measured configuration: ns per accel_field call over the stored arc.
 *
 * The accelerations are summed and returned so that nothing here can be
 * optimised away, and so the caller can print the sum: four configurations
 * that produced identical sums would mean the terms were never switched on.
 */
static double time_field(FieldCtx *field, const State *arc, long *calls_out,
                         double *checksum_out)
{
    double checksum = 0.0;
    long calls = 0;
    double start = now_s();
    double elapsed = 0.0;

    while (elapsed < BUDGET_S) {
        for (int i = 0; i < SAMPLES; i++) {
            Vec3d a;
            accel_field(arc[i].t, arc[i].r, arc[i].v, field, &a);
            checksum += a.x + a.y + a.z;
        }
        calls += SAMPLES;
        elapsed = now_s() - start;
    }

    *calls_out = calls;
    *checksum_out = checksum;
    return elapsed * 1e9 / (double)calls;
}

/* Terms of degree 2 and up - what the recursion walks. Degree 0 is the point
 * mass and degree 1 vanishes in a centre-of-mass frame (core/harmonics.h). */
static int harmonics_terms(int degree)
{
    return (degree + 1) * (degree + 2) / 2 - 3;
}

/* How the Pines recursion scales with degree, and what that says about K5.
 *
 * The measurement above found harmonics nearly free (+3% for the Earth's J2)
 * and named the reason: the cost is the ephemeris, not the recursion. That
 * conclusion has a range of validity, and K5 is outside it - a Moon at
 * roughly degree 50 walks 1323 terms rather than three, and the term count is
 * quadratic in degree while everything else in the model is linear in the
 * number of bodies.
 *
 * So the slope is measured here rather than left to be discovered by K5. It
 * cannot be measured AT degree 50: HARMONICS_MAX_DEGREE is 8 until K5
 * rewrites the recursion normalised, and the rewrite may move the constant.
 * What is measured is the shape - cost per term - and the extrapolation is
 * printed as an extrapolation, in the one place that will produce the real
 * number the moment the ceiling rises. */
static void harmonics_scaling(void)
{
    static HarmonicsField f;
    Vec3d r = vec3(4.1e6, 3.2e6, 2.7e6);
    double mu = 3.986004418e14;
    double first_ns = 0.0;
    double last_ns = 0.0;
    int degree;

    f.re = 6378136.3;
    for (int i = 0; i < HARMONICS_MAX_COEFFS; i++) {
        /* Realistic magnitudes; the cost is in the recursion's shape and not
         * in what the coefficients say. */
        f.c[i] = 1e-6 / (double)(i + 1);
        f.s[i] = 5e-7 / (double)(i + 2);
    }

    printf("\n  Pines recursion by degree (ceiling is %d until K5):\n",
           HARMONICS_MAX_DEGREE);

    for (degree = 2; degree <= HARMONICS_MAX_DEGREE; degree++) {
        Vec3d sink = vec3(0.0, 0.0, 0.0);
        Vec3d probe = r;
        long calls = 0;
        double elapsed = 0.0;
        double start = now_s();
        double ns;

        f.degree = degree;
        while (elapsed < BUDGET_S) {
            for (int i = 0; i < SAMPLES; i++) {
                Vec3d a;
                /* Moving the point keeps the call from being hoisted. */
                probe.x += 1e-9;
                harmonics_accel(&f, probe, mu, &a);
                sink = vec3_add(sink, a);
            }
            calls += SAMPLES;
            elapsed = now_s() - start;
        }
        ns = elapsed * 1e9 / (double)calls;

        if (degree == 2) {
            first_ns = ns;
        }
        last_ns = ns;

        printf("    degree %2d  terms %4d  %7.1f ns/call  (sink %.3e)\n",
               degree, harmonics_terms(degree), ns, sink.x);
    }

    {
        int first_terms = harmonics_terms(2);
        int last_terms = harmonics_terms(HARMONICS_MAX_DEGREE);
        double slope = (last_ns - first_ns)
                       / (double)(last_terms - first_terms);
        double base = first_ns - slope * (double)first_terms;
        int k5_terms = harmonics_terms(K5_DEGREE);

        printf("    fit %.2f ns/term + %.1f ns;"
               " EXTRAPOLATED to degree %d (%d terms): %.0f ns/call\n",
               slope, base, K5_DEGREE, k5_terms,
               base + slope * (double)k5_terms);
        printf("    not a measurement - see the comment above this table\n");
    }
}

static void report(const char *label, double ns, double base, long calls,
                   double checksum)
{
    printf("  %-22s %8.1f ns", label, ns);
    if (base > 0.0) {
        printf("   %+6.1f%% vs point masses", 100.0 * (ns - base) / base);
    }
    printf("\n        %ld calls, checksum %.17g\n", calls, checksum);
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "bench_field: cannot load %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` builds it\n");
        return 1;
    }

    FieldCtx field;
    if (field_all_bodies(eph, &field) != CORE_OK) {
        fprintf(stderr, "bench_field: cannot build the field\n");
        eph_free(eph);
        return 1;
    }

    /* Cr and Cd of a plain box-and-panel spacecraft, the same order as the
     * vessel K7b flew: Cd * A / m = 0.022 m^2/kg. The absolute values do not
     * change the cost, only whether the term runs at all - but a vessel with
     * an implausible area would make the printed accelerations useless as a
     * sanity check. */
    VesselParams vessel;
    memset(&vessel, 0, sizeof vessel);
    vessel.mass_kg = 1000.0;
    vessel.area_m2 = 10.0;
    vessel.cr = 1.5;
    vessel.cd = 2.2;

    State earth;
    if (eph_body_state(eph, EARTH, 0.0, &earth) != CORE_OK) {
        fprintf(stderr, "bench_field: cannot read the Earth\n");
        eph_free(eph);
        return 1;
    }

    State s;
    s.t = 0.0;
    s.r.x = earth.r.x + OFFSET_R.x;
    s.r.y = earth.r.y + OFFSET_R.y;
    s.r.z = earth.r.z + OFFSET_R.z;
    s.v.x = earth.v.x + OFFSET_V.x;
    s.v.y = earth.v.y + OFFSET_V.y;
    s.v.z = earth.v.z + OFFSET_V.z;

    /* The arc, integrated under the full model - the same one configuration
     * four below will time. Outside the timed region, so using accel_field to
     * produce the states it is then measured on is not circular. */
    field_set_vessel(&field, &vessel);

    Dop853Config icfg;
    memset(&icfg, 0, sizeof icfg);
    icfg.tol_m = 1e-3;
    icfg.h_max = SAMPLE_DT_S;

    Dop853State ist;
    memset(&ist, 0, sizeof ist);

    static State arc[SAMPLES];
    for (int i = 0; i < SAMPLES; i++) {
        State next;
        double t_end = SAMPLE_DT_S * (double)(i + 1);

        if (dop853_integrate(accel_field, &field, &s, t_end, &icfg, &ist,
                             &next) != CORE_OK || field.failed) {
            fprintf(stderr, "bench_field: cannot build the arc at sample %d\n", i);
            eph_free(eph);
            return 1;
        }
        s = next;
        arc[i] = next;
    }

    printf("bench_field: %s, %d bodies, %d states over %.0f s\n", ASSET,
           field.n_bodies, SAMPLES, SAMPLE_DT_S * SAMPLES);
    printf("  vessel: %.0f kg, %.0f m^2, cr %.1f, cd %.1f; altitude %.0f km\n",
           vessel.mass_kg, vessel.area_m2, vessel.cr, vessel.cd,
           ALTITUDE_M / 1e3);
    printf("  one accel_field evaluation:\n");

    /* One statement per measurement, never report(..., time_field(...), ...,
     * calls, checksum): the arguments of one call are unsequenced, so reading
     * the two outputs in the same expression that writes them is undefined
     * behaviour. It is also undefined behaviour that LOOKS like a result -
     * the first version of this file printed the previous configuration's
     * count and checksum for every row, and two identical checksums read as
     * "the term does nothing" rather than as a bug in the benchmark. */
    long calls;
    double checksum;

    /* Point masses: harmonics dropped, and a vessel that asks for neither
     * radiation pressure nor drag - which is bit-for-bit the field this file
     * produced before K4b, K6b and K7b respectively. */
    FieldCtx bare = field;
    field_clear_harmonics(&bare);
    field_set_vessel(&bare, NULL);
    double base = time_field(&bare, arc, &calls, &checksum);
    report("point masses", base, 0.0, calls, checksum);

    FieldCtx with_harmonics = field;
    field_set_vessel(&with_harmonics, NULL);
    double ns_harmonics = time_field(&with_harmonics, arc, &calls, &checksum);
    report("+ harmonics", ns_harmonics, base, calls, checksum);

    VesselParams sunlit = vessel;
    sunlit.cd = 0.0;
    FieldCtx with_srp = field;
    field_set_vessel(&with_srp, &sunlit);
    double ns_srp = time_field(&with_srp, arc, &calls, &checksum);
    report("+ SRP", ns_srp, base, calls, checksum);

    /* The full model, vessel intact: this is the line the game pays. */
    double ns_full = time_field(&field, arc, &calls, &checksum);
    report("+ drag (full model)", ns_full, base, calls, checksum);

    /* The number D6 was actually about.
     *
     * bench_dop853 prints "vessel-steps that fit a 60 Hz tick" from
     * accel_two_body, and that line has been read as a capacity figure for
     * the game since it was written. It is not one: DOP853 spends fifteen
     * acceleration evaluations per accepted step (that ratio is the
     * integrator's, and holds whatever the force model), so a step under the
     * model the game flies costs fifteen of the line above, not fifteen of a
     * two-body evaluation.
     *
     * Deliberately an estimate assembled from two benchmarks rather than a
     * third measurement. Running DOP853 on accel_field here would measure a
     * particular orbit's step sizes as much as the force model, and the arc
     * above was integrated at a tolerance chosen to produce samples, not to
     * represent a mission. The ratio, though, is what turns one number into
     * the other, and printing it is what stops the cheap line being quoted
     * for the expensive question. */
    const double evals_per_step = 15.0;
    double us_per_step = ns_full * evals_per_step / 1e3;

    printf("\n  at %.0f evals per accepted step, one vessel-step costs %.1f us:\n",
           evals_per_step, us_per_step);
    printf("    vessel-steps in a 60 Hz tick (16.7 ms)  %.0f\n",
           16667.0 / us_per_step);
    printf("    vessel-steps in a 30 Hz tick (33.3 ms)  %.0f\n",
           33333.0 / us_per_step);
    /* The reference bench_dop853 runs, timed here on the same states and the
     * same machine, because a ratio assembled from two binaries' output is a
     * ratio nobody measured. This is the whole of D6 in one line: the older
     * benchmark's number is this one, and the game pays the one above it. */
    TwoBodyCtx two = { 3.98600435436e14 };
    long two_calls;
    double two_checksum = 0.0;
    double two_elapsed = 0.0;
    double two_start = now_s();
    long two_total = 0;

    while (two_elapsed < BUDGET_S) {
        for (int i = 0; i < SAMPLES; i++) {
            Vec3d a;
            accel_two_body(arc[i].t, arc[i].r, arc[i].v, &two, &a);
            two_checksum += a.x + a.y + a.z;
        }
        two_total += SAMPLES;
        two_elapsed = now_s() - two_start;
    }
    two_calls = two_total;
    double ns_two = two_elapsed * 1e9 / (double)two_calls;

    printf("\n  accel_two_body on the same states: %.1f ns\n", ns_two);
    printf("        %ld calls, checksum %.17g\n", two_calls, two_checksum);
    printf("  the full model costs %.0f times that per evaluation\n",
           ns_full / ns_two);

    /* ---- The Moon with its mascons (ROADMAP K5e) ----------------------- *
     *
     * Everything above is measured in Earth orbit, where the only harmonic
     * term is J2 and the harmonics cost is two rows of a recursion. The
     * lunar model is fifty rows and 1323 terms, and it is the reason the
     * degree ceiling moved at all, so what it costs a vessel that is
     * actually near the Moon belongs in the same table rather than in a
     * commit message.
     *
     * The comparison is against the same field with the harmonics dropped -
     * field_clear_harmonics - so the difference is the lunar model and not
     * the ten bodies underneath it. */
    {
        State moon;
        if (eph_body_state(eph, MOON, 0.0, &moon) != CORE_OK) {
            fprintf(stderr, "bench_field: cannot read the Moon\n");
            eph_free(eph);
            return 1;
        }

        double radius = eph_body_radius(eph, MOON) + 100.0e3;
        double speed = sqrt(eph_body_mu(eph, MOON) / radius);

        static State lunar[SAMPLES];
        State ls;
        ls.t = 0.0;
        ls.r = vec3(moon.r.x + radius * 0.8, moon.r.y + radius * 0.6,
                    moon.r.z);
        ls.v = vec3(moon.v.x - speed * 0.6, moon.v.y + speed * 0.8,
                    moon.v.z);

        FieldCtx lunar_field;
        if (field_all_bodies(eph, &lunar_field) != CORE_OK) {
            eph_free(eph);
            return 1;
        }

        Dop853Config lcfg;
        memset(&lcfg, 0, sizeof lcfg);
        lcfg.tol_m = 1e-3;
        lcfg.h_max = 10.0;
        Dop853State lst;
        memset(&lst, 0, sizeof lst);

        for (int i = 0; i < SAMPLES; i++) {
            State next;
            if (dop853_integrate(accel_field, &lunar_field, &ls,
                                 10.0 * (double)(i + 1), &lcfg, &lst, &next)
                    != CORE_OK || lunar_field.failed) {
                fprintf(stderr, "bench_field: cannot build the lunar arc\n");
                eph_free(eph);
                return 1;
            }
            ls = next;
            lunar[i] = next;
        }

        FieldCtx bare_lunar = lunar_field;
        field_clear_harmonics(&bare_lunar);

        long calls;
        double sink;
        double ns_bare = time_field(&bare_lunar, lunar, &calls, &sink);
        double ns_grail = time_field(&lunar_field, lunar, &calls, &sink);

        printf("\n  100 km over the Moon, degree %d GRAIL field:\n",
               eph_body_harmonics(eph, MOON)->degree);
        printf("    point masses only        %8.1f ns\n", ns_bare);
        printf("    with the lunar field     %8.1f ns   %+.0f%%\n", ns_grail,
               100.0 * (ns_grail - ns_bare) / ns_bare);
    }

    harmonics_scaling();

    if (field.failed) {
        fprintf(stderr, "bench_field: the field reported a failure\n");
        eph_free(eph);
        return 1;
    }

    eph_free(eph);
    return 0;
}
