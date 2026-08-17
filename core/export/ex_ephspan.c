/* Export: what a longer ephemeris asset costs and what it buys.
 *
 *   ephspan.csv   divergence from JPL Horizons, measured through the asset,
 *                 for a doubling sequence of asset spans
 *
 * ROADMAP's first fork ("Дві розвилки, що змінюють план цілком" - "Ефемерида
 * не тримає точності на 200 років") was
 * measured out to 200 years on a two-body round trip and deliberately left
 * open, with one concrete objection recorded against deciding it: the shipped
 * fixture spans 120 days, so nothing in the repository ever exercised a long
 * asset. Swapping DOP853 for IAS15 in the cooker cannot be justified by a
 * number that no fixture reaches.
 *
 * This removes that objection at the only horizon the repository can defend.
 * The cooker is run at 120, 240, 480, 960, 1920 days and finally the full
 * span of data/horizons, and each asset is read back through eph_body_state -
 * the runtime's own path, not the integrator's internal state - and compared
 * with JPL at every reference epoch it covers.
 *
 * The upper limit is not a preference. Past the last Horizons epoch there is
 * nothing left to be wrong against, and a curve with no oracle beneath it
 * would measure self-consistency, which max_fit_error_m already measures.
 * So the sequence doubles until it would overshoot the oracle and then lands
 * exactly on it, and the number comes from the loaded data rather than from a
 * constant here that could drift away from data/horizons unnoticed.
 *
 * Interval length, degree and integrator tolerance are held at the committed
 * fixture's values. Span is the only variable, which is what makes the rows
 * comparable at all.
 *
 * Two things are measured that a single long cook would not show:
 *
 *   cost     intervals, bytes and integrator steps against span, which is the
 *            practical question if the shipped fixture is ever extended
 *   prefix   whether a longer asset merely extends a shorter one or is a
 *            different asset over the shared part. Interval k is fitted from
 *            the same forced landings whatever the total span, so it should
 *            be bit-identical - but "should" is how silent asset changes get
 *            through, and every determinism hash that reads the fixture is
 *            downstream of the answer. It is checked, not assumed.
 *
 *            One epoch per span is exempt from that check and reported apart:
 *            the span's own end. A closing interval's polynomial is evaluated
 *            there at its right edge, while in a longer asset the same instant
 *            falls at the left edge of the interval after it. Two polynomials,
 *            one instant, agreeing only to the fit error - that is the seam
 *            between intervals, present in every asset at every interval
 *            boundary, and nothing to do with where the cook stopped.
 *
 * The raw ten-body integration is exported alongside as a control: same
 * physics, no asset in between. If the asset's error tracks it, the
 * divergence is the integrator's and the fork is about IAS15; if the asset
 * is worse, the Chebyshev layer dominates and a better integrator buys
 * nothing.
 *
 * It runs twice, at TOLERANCE_M and at TOLERANCE_LOOSE_M, and the second run
 * is history kept deliberately. The cooker lands on every fit node, so it
 * takes far shorter steps than a control landing only on reference epochs; at
 * the metre the fixture used until this file was written, the two were not
 * the same integration at all, and the gap was easy to read as the asset
 * being wrong when it was the control that had not converged. Exporting both
 * is what tells those apart, and the tolerance sweep printed at the end is
 * what the fixture's current tolerance was chosen from.
 *
 * Offline code: the mutual N-body integration the cooker runs, never the
 * runtime (core/offline/nbody.h). Run from the repository root. */

#include "csv.h"
#include "eph_build.h"
#include "ephemeris.h"
#include "nbody.h"
#include "refdata.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define MAX_SAMPLES 256
#define MAX_SPANS 8
#define DAY 86400.0
#define YEAR (365.25 * DAY)

/* Every major body, in the order and with the parameters cook_fixture.c uses.
 * Diverging from it here would measure this file rather than the fixture. */
/* Radii and the solar flux are cited, never invented - the rule
 * data/horizons/README.md states and K2 followed for J2. Each number is the
 * volumetric mean radius from that body's own already-committed Horizons
 * page, in metres:
 *
 *   sun 695700 km (also the IAU 2015 solar radius), mercury 2439.4,
 *   venus 6051.84, earth 6371.01, moon 1737.53, mars 3389.92,
 *   jupiter 69911, saturn 58232, uranus 25362, neptune 24624.
 *
 * Mean rather than equatorial, deliberately: this radius draws a shadow, and
 * a shadow is cast by the whole disc. It is a different number from the
 * reference radius of the harmonic field below (6378137 m, equatorial), which
 * is a scale in an expansion rather than a size, and the two must not be
 * confused into one field.
 *
 * The outer four bodies are barycentres, so this is the planet's radius
 * standing in for the system's. The error is the planet's offset from its own
 * barycentre - largest for Jupiter, and there it is under a thousandth of the
 * radius itself.
 *
 * The Sun's flux is the "Solar constant (1 AU)" line of obj_sun.txt, 1367.6
 * W/m^2. Modern total solar irradiance measurements put it near 1361, and
 * PROJECT.md section 7 quotes that figure for exposure; the half a per cent
 * between them is far inside the uncertainty of Cr, and citing the file we
 * committed beats quoting a better number from memory. When the page is
 * refreshed the number follows it. */
static HarmonicsField earth_shape;

static const EphBodyInfo BODIES[] = {
    { "sun", 6.957e8, 1367.6, NULL, NULL },
    { "mercury", 2.4394e6, 0.0, NULL, NULL },
    { "venus", 6.05184e6, 0.0, NULL, NULL },
    { "earth", 6.37101e6, 0.0, NULL, NULL },
    { "moon", 1.73753e6, 0.0, NULL, NULL },
    { "mars_bary", 3.38992e6, 0.0, NULL, NULL },
    { "jupiter_bary", 6.9911e7, 0.0, NULL, NULL },
    { "saturn_bary", 5.8232e7, 0.0, NULL, NULL },
    { "uranus_bary", 2.5362e7, 0.0, NULL, NULL },
    { "neptune_bary", 2.4624e7, 0.0, NULL, NULL },
};
#define N_BODIES (sizeof BODIES / sizeof BODIES[0])

#define START_DAYS 120.0
#define INTERVAL_DAYS 8.0
#define DEGREE 14

/* Tracks cook_fixture.c deliberately: this file exists to say what the
 * shipped asset does, and a tolerance of its own would answer about something
 * nobody ships. */
#define TOLERANCE_M 1.0e-6

/* What the fixture used before ex_ephspan measured it. Kept as a control
 * rather than deleted: the difference between the two rows is the finding,
 * and a number that only exists in a commit message stops being checked. */
#define TOLERANCE_LOOSE_M 1.0

/* Overwritten once per span. Only one asset is needed at a time, and the
 * longest is 1.5 MB - worth not leaving six of them in build/. */
static const char *SCRATCH_PATH = "build/ephspan.eph";

static RefSample reference[N_BODIES][MAX_SAMPLES];
static size_t n_samples;
static RefGm gm_table[16];
static size_t n_gm;

static int earth_idx = -1;
static int moon_idx = -1;

/* Errors at every epoch of every span, kept so the prefix check can compare a
 * span against the longest one afterwards. [span][sample][earth, rel, moon] */
static double errors[MAX_SPANS][MAX_SAMPLES][3];
static size_t n_epochs[MAX_SPANS];

typedef struct {
    double days;
    size_t intervals;
    size_t bytes;
    long   steps;
    double fit_error_m;
    double earth_m;
    double earth_rel_m;
    double moon_geo_m;
} SpanSummary;

static SpanSummary summary[MAX_SPANS];
static double spans[MAX_SPANS];
static size_t n_spans;

static int load_fixtures(void)
{
    char path[128];

    for (size_t i = 0; i < N_BODIES; i++) {
        snprintf(path, sizeof path, "data/horizons/vec_%s.csv", BODIES[i].name);
        size_t n = 0;
        if (refdata_load_vectors(path, reference[i], MAX_SAMPLES, &n)
            != CORE_OK) {
            fprintf(stderr, "ex_ephspan: cannot load %s\n", path);
            return 0;
        }
        if (i == 0) {
            n_samples = n;
        } else if (n != n_samples) {
            fprintf(stderr, "ex_ephspan: %s has %zu samples, expected %zu\n",
                    path, n, n_samples);
            return 0;
        }

        if (strcmp(BODIES[i].name, "earth") == 0) {
            earth_idx = (int)i;
        }
        if (strcmp(BODIES[i].name, "moon") == 0) {
            moon_idx = (int)i;
        }
    }

    if (earth_idx < 0 || moon_idx < 0) {
        return 0;
    }

    return refdata_load_gm("data/horizons/gm.csv", gm_table, 16, &n_gm)
           == CORE_OK;
}

/* Doubling from the committed fixture's span, stopping on the oracle rather
 * than past it. */
static void build_span_list(double oracle_days)
{
    n_spans = 0;
    for (double d = START_DAYS;
         d < oracle_days && n_spans + 1 < MAX_SPANS;
         d *= 2.0) {
        spans[n_spans++] = d;
    }
    spans[n_spans++] = oracle_days;
}

static int fill_system(NBodySystem *sys)
{
    memset(sys, 0, sizeof *sys);
    sys->n = N_BODIES;

    for (size_t i = 0; i < N_BODIES; i++) {
        sys->mu[i] = refdata_gm_of(gm_table, n_gm, BODIES[i].name);
        if (!(sys->mu[i] > 0.0)) {
            fprintf(stderr, "ex_ephspan: no GM for %s\n", BODIES[i].name);
            return 0;
        }
    }

    /* Same J2 as cook_fixture.c - tracking it deliberately, like BODIES[]
     * above, because this file exists to measure what the shipped fixture
     * does (ROADMAP K2). Values cited in cook_fixture.c's own comment. */
    /* The same cited J2 the cooker uses, through the same setter (K5b), and
     * held at file scope because NBodySystem borrows it (K5e). This
     * diagnostic deliberately does NOT give the Moon its GRAIL field: it
     * measures how the asset's accuracy grows with span, and the lunar
     * mascons move nothing at inter-body distances - adding them would only
     * make this slower to run and harder to compare with its own history. */
    earth_shape.degree = 2;
    earth_shape.re = 6378137.0;
    harmonics_set_unnormalised(&earth_shape, 2, 0, -1.08262545e-3, 0.0);
    sys->field[earth_idx] = &earth_shape;

    return 1;
}

/* The three measures ex_horizons settled on, for the same reasons: the
 * barycentric error carries any bulk drift of the modelled subsystem, the
 * relative one is genuine distortion of the orbits, and the geocentric Moon
 * is the geometry the game is built on. */
static void compare(const NBodySystem *sys, const State *model, size_t sample,
                    double out[3])
{
    State ref_now[NBODY_MAX];
    for (size_t i = 0; i < N_BODIES; i++) {
        ref_now[i] = reference[i][sample].s;
    }

    Vec3d bary_model = nbody_barycentre(sys, model);
    Vec3d bary_ref = nbody_barycentre(sys, ref_now);

    out[0] = vec3_distance(model[earth_idx].r, ref_now[earth_idx].r);
    out[1] = vec3_distance(vec3_sub(model[earth_idx].r, bary_model),
                           vec3_sub(ref_now[earth_idx].r, bary_ref));
    out[2] = vec3_distance(vec3_sub(model[moon_idx].r, model[earth_idx].r),
                           vec3_sub(ref_now[moon_idx].r, ref_now[earth_idx].r));
}

static int measure_span(Csv *c, size_t slot, const NBodySystem *sys)
{
    double span_days = spans[slot];
    char label[32];
    snprintf(label, sizeof label, "asset_%.0fd", span_days);

    State initial[NBODY_MAX];
    for (size_t i = 0; i < N_BODIES; i++) {
        initial[i] = reference[i][0].s;
    }

    EphBuildConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.t_begin = 0.0;
    cfg.t_end = span_days * DAY;
    cfg.interval_seconds = INTERVAL_DAYS * DAY;
    cfg.degree = DEGREE;
    cfg.tol_m = TOLERANCE_M;

    EphBuildReport report;
    memset(&report, 0, sizeof report);

    if (eph_build(sys, initial, BODIES, &cfg, SCRATCH_PATH, &report)
        != CORE_OK) {
        fprintf(stderr, "ex_ephspan: build failed at %.0f days\n", span_days);
        return 0;
    }

    /* Read back through the runtime's path. Measuring the integrator's own
     * states instead would skip the Chebyshev layer, which is half of what
     * the asset is. */
    EphemerisCtx *ctx = NULL;
    if (eph_load(SCRATCH_PATH, &ctx) != CORE_OK) {
        fprintf(stderr, "ex_ephspan: cannot load %s\n", SCRATCH_PATH);
        return 0;
    }

    size_t count = 0;

    for (size_t s = 0; s < n_samples; s++) {
        double t = reference[0][s].s.t;
        if (t > cfg.t_end) {
            break;
        }

        State model[NBODY_MAX];
        int ok = 1;
        for (size_t i = 0; i < N_BODIES && ok; i++) {
            ok = eph_body_state(ctx, (int)i, t, &model[i]) == CORE_OK;
        }
        if (!ok) {
            fprintf(stderr, "ex_ephspan: %s has no state at %.0f days\n",
                    label, t / DAY);
            eph_free(ctx);
            return 0;
        }

        compare(sys, model, s, errors[slot][count]);

        csv_named(c, label, 4, t / DAY,
                  errors[slot][count][0],
                  errors[slot][count][1],
                  errors[slot][count][2]);
        count++;
    }

    eph_free(ctx);

    if (count == 0) {
        return 0;
    }
    n_epochs[slot] = count;

    summary[slot].days = span_days;
    summary[slot].intervals = report.intervals;
    summary[slot].bytes = report.bytes_written;
    summary[slot].steps = report.integrator_steps;
    summary[slot].fit_error_m = report.max_fit_error_m;
    summary[slot].earth_m = errors[slot][count - 1][0];
    summary[slot].earth_rel_m = errors[slot][count - 1][1];
    summary[slot].moon_geo_m = errors[slot][count - 1][2];

    return 1;
}

/* The same ten bodies integrated straight to each reference epoch, with no
 * asset in between. `label` and `tol_m` vary because the control has to be
 * run at both tolerances to be readable at all - see the header. Rows go to
 * the CSV only when `c` is given; the sweep below reuses this for its final
 * numbers and wants no curves. */
static int measure_raw(Csv *c, const NBodySystem *sys, const char *label,
                       double tol_m, double out[3], long *steps_out)
{
    State current[NBODY_MAX];
    for (size_t i = 0; i < N_BODIES; i++) {
        current[i] = reference[i][0].s;
    }

    /* Whatever eph_build does to its own copy, so that "the asset agrees with
     * the control" keeps meaning what it says. */
    if (eph_anchor_enabled()) {
        nbody_anchor_barycentre(sys, current);
    }

    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol_m;
    cfg.max_steps = 50000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    if (c != NULL) {
        csv_named(c, label, 4, 0.0, 0.0, 0.0, 0.0);
    }

    for (size_t s = 1; s < n_samples; s++) {
        State next[NBODY_MAX];
        if (nbody_integrate(sys, current, reference[0][s].s.t, &cfg, &st, next)
            != CORE_OK) {
            fprintf(stderr, "ex_ephspan: %s stopped at sample %zu\n", label, s);
            return 0;
        }
        memcpy(current, next, sizeof next);

        compare(sys, current, s, out);
        if (c != NULL) {
            csv_named(c, label, 4, reference[0][s].s.t / DAY,
                      out[0], out[1], out[2]);
        }
    }

    if (steps_out != NULL) {
        *steps_out = st.n_accepted;
    }
    return 1;
}

/* Why the control is run twice, as a table rather than as an assertion: the
 * tolerance at which the ten-year answer stops moving is a property of the
 * system, not something to assume. It is also the number that says whether
 * the fixture's 1 m has any margin left at the Moon. */
static int sweep_tolerance(const NBodySystem *sys)
{
    static const double tols[] = { 1.0, 1e-1, 1e-2, 1e-3, 1e-4, 1e-5 };

    printf("\n  control at falling tolerance, at the last epoch:\n");
    printf("  %10s %8s %13s %13s\n",
           "tol_m", "steps", "earth_rel_m", "moon_geo_m");

    for (size_t i = 0; i < sizeof tols / sizeof tols[0]; i++) {
        double e[3] = { 0.0, 0.0, 0.0 };
        long steps = 0;
        if (!measure_raw(NULL, sys, "sweep", tols[i], e, &steps)) {
            return 0;
        }
        printf("  %10.0e %8ld %13.5g %13.5g\n", tols[i], steps, e[1], e[2]);
    }
    return 1;
}

/* How fast the divergence grows, as the exponent k in error ~ t^k, by least
 * squares on log t against log error.
 *
 * This is the number the whole fork turns on and it belongs here rather than
 * in the plotting script, which deliberately computes nothing. A
 * non-symplectic integrator's roundoff shows up as k = 2 - that is exactly
 * what ex_accuracy.c's reversibility diagnostic measures over 200 years, a
 * round-trip error growing quadratically. A constant missing force shows up
 * as k = 1. The two mechanisms are told apart by this one number, and no
 * amount of looking at the size of the error would separate them.
 *
 * from_days skips the start, where the error is still climbing out of the
 * initial condition and the exponent means nothing yet. */
static double growth_exponent(const double values[MAX_SAMPLES][3], size_t n,
                              int measure, double from_days)
{
    double sx = 0.0, sy = 0.0, sxx = 0.0, sxy = 0.0;
    size_t used = 0;

    for (size_t s = 0; s < n; s++) {
        double days = reference[0][s].s.t / DAY;
        double v = values[s][measure];
        if (days < from_days || !(v > 0.0)) {
            continue;
        }

        double x = log(days), y = log(v);
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
        used++;
    }

    if (used < 2) {
        return 0.0;
    }

    double dn = (double)used;
    double denominator = dn * sxx - sx * sx;
    if (denominator == 0.0) {
        return 0.0;
    }
    return (dn * sxy - sx * sy) / denominator;
}

/* Every shorter span against the longest one, epoch by epoch, as exact
 * equality - except at the span's own end, where the seam described in the
 * header makes exact equality the wrong question and the size of the
 * disagreement the right one.
 *
 * An interior mismatch would mean the cook depends on where it stops, which
 * would make extending the fixture a rewrite of its whole history rather than
 * an addition to its end. */
static void prefix_check(size_t *interior_bad, double *seam_max_m)
{
    size_t last = n_spans - 1;

    *interior_bad = 0;
    *seam_max_m = 0.0;

    for (size_t k = 0; k < last; k++) {
        for (size_t s = 0; s < n_epochs[k]; s++) {
            int at_seam = reference[0][s].s.t == spans[k] * DAY;

            for (int m = 0; m < 3; m++) {
                double d = errors[k][s][m] - errors[last][s][m];
                if (d == 0.0) {
                    continue;
                }
                if (at_seam) {
                    double a = d < 0.0 ? -d : d;
                    if (a > *seam_max_m) {
                        *seam_max_m = a;
                    }
                } else {
                    (*interior_bad)++;
                }
            }
        }
    }
}

int main(void)
{
    if (!load_fixtures()) {
        fprintf(stderr, "  run from the repository root\n");
        return 1;
    }

    NBodySystem sys;
    if (!fill_system(&sys)) {
        return 1;
    }

    double oracle_days =
        (reference[0][n_samples - 1].s.t - reference[0][0].s.t) / DAY;
    build_span_list(oracle_days);

    printf("ex_ephspan: oracle is %.0f days (%.1f years) in %zu epochs\n",
           oracle_days, oracle_days * DAY / YEAR, n_samples);
    printf("  fixed: interval %.0f days, degree %d, tol %g m, barycentre %s\n",
           INTERVAL_DAYS, DEGREE, TOLERANCE_M,
           eph_anchor_enabled() ? "anchored" : "NOT anchored");

    Csv c;
    if (!csv_open(&c, "build/csv/ephspan.csv",
                  "source,days,earth_m,earth_rel_m,moon_geo_m")) {
        return 1;
    }

    for (size_t k = 0; k < n_spans; k++) {
        if (!measure_span(&c, k, &sys)) {
            return 1;
        }
    }

    double raw_loose[3] = { 0.0, 0.0, 0.0 };
    double raw_tight[3] = { 0.0, 0.0, 0.0 };
    long raw_loose_steps = 0, raw_tight_steps = 0;

    if (!measure_raw(&c, &sys, "raw_tol_1m", TOLERANCE_LOOSE_M,
                     raw_loose, &raw_loose_steps)
        || !measure_raw(&c, &sys, "raw_fixture_tol", TOLERANCE_M,
                        raw_tight, &raw_tight_steps)) {
        return 1;
    }

    printf("\n");
    printf("  %13s %6s %5s %8s %8s %11s %11s %11s %11s\n",
           "asset span", "years", "intv", "kB", "steps", "fit_err_m",
           "earth_m", "earth_rel_m", "moon_geo_m");

    for (size_t k = 0; k < n_spans; k++) {
        char span_label[16];
        snprintf(span_label, sizeof span_label, "%.0f d", summary[k].days);

        printf("  %13s %6.2f %5zu %8.1f %8ld %11.4g %11.4g %11.4g %11.4g\n",
               span_label, summary[k].days * DAY / YEAR,
               summary[k].intervals, (double)summary[k].bytes / 1024.0,
               summary[k].steps, summary[k].fit_error_m,
               summary[k].earth_m, summary[k].earth_rel_m,
               summary[k].moon_geo_m);
    }

    char loose_label[32], tight_label[32];
    snprintf(loose_label, sizeof loose_label, "ctrl %g m", TOLERANCE_LOOSE_M);
    snprintf(tight_label, sizeof tight_label, "ctrl %g m", TOLERANCE_M);

    printf("  %13s %6.2f %5s %8s %8ld %11s %11.4g %11.4g %11.4g\n",
           loose_label, oracle_days * DAY / YEAR, "-", "-",
           raw_loose_steps, "-",
           raw_loose[0], raw_loose[1], raw_loose[2]);
    printf("  %13s %6.2f %5s %8s %8ld %11s %11.4g %11.4g %11.4g\n",
           tight_label, oracle_days * DAY / YEAR, "-", "-",
           raw_tight_steps, "-",
           raw_tight[0], raw_tight[1], raw_tight[2]);

    size_t last = n_spans - 1;
    printf("\n  growth exponent k in error ~ t^k, full asset, from one year on:\n");
    printf("    earth_rel  k = %.2f\n",
           growth_exponent(errors[last], n_epochs[last], 1, 365.0));
    printf("    moon_geo   k = %.2f\n",
           growth_exponent(errors[last], n_epochs[last], 2, 365.0));
    printf("  (k=1 a constant missing force, k=2 a non-symplectic "
           "integrator's roundoff)\n");

    if (!sweep_tolerance(&sys)) {
        return 1;
    }

    size_t interior_bad = 0;
    double seam_max_m = 0.0;
    prefix_check(&interior_bad, &seam_max_m);

    printf("\n  prefix: %s\n",
           interior_bad == 0
               ? "every shorter span is bit-identical to the longest"
               : "SPANS DISAGREE - a longer asset is not an extension");
    if (interior_bad != 0) {
        printf("  %zu differing values away from any interval seam\n",
               interior_bad);
    }
    printf("  seam:   %.3g m, largest disagreement at a span's own end\n",
           seam_max_m);

    return csv_close(&c) ? 0 : 1;
}
