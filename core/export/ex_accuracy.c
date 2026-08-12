/* Export: what the integrator loses, as curves (Milestone 0 delivery).
 *
 * Two of the six acceptance criteria are numbers whose value means little on
 * its own and a great deal against a trend, and a trend is a plot:
 *
 *   jacobi.csv          conserved quantity lost over 100 revolutions, against
 *                       the tolerance asked for and against time
 *   reversibility.csv   out and back over one to ten years
 *
 * The point of the first is not that the drift is small. It is that the drift
 * follows the tolerance nearly one for one, over eight orders of magnitude.
 * A drift that stopped improving when the tolerance was tightened would mean
 * the loss was structural - a wrong force term, a broken step controller - and
 * no single measurement can tell those apart. The straight line can.
 *
 * The second is the same argument in time rather than in tolerance. A
 * round-trip error growing linearly with the span is rounding; growing faster
 * than the span is a method that is losing the trajectory.
 *
 * Run from the repository root. */

#include "accel.h"
#include "cr3bp.h"
#include "csv.h"
#include "integrator.h"

#include <math.h>
#include <string.h>

/* GM values from data/horizons/gm.csv, as core/test/test_cr3bp.c uses them. */
#define GM_EARTH 398600.435436
#define GM_MOON  4902.800066

#define MU_EARTH 3.98600435436e14
#define R_LUNAR 3.844e8
#define YEAR (365.25 * 86400.0)

#define TWO_PI 6.28318530717958647692

#define REVOLUTIONS 100
#define GROWTH_TOLERANCE 1e-12
#define MAX_YEARS 10

/* The two starting states core/test/test_cr3bp.c measures, kept identical so
 * the plot and the assertion are about the same thing.
 *
 * They are here together because one alone would mislead. The L4 case is a
 * gentle orbit and gives the integrator's best behaviour; the second passes
 * far closer to the primaries and drifts three orders of magnitude more at the
 * same tolerance. That gap is a property of the orbit, not of the method, and
 * without the second curve the first would be read as the method's. */
typedef struct {
    const char *name;
    State       start;
} Case;

static double tolerances[] = { 1e-6, 1e-8, 1e-10, 1e-12, 1e-14 };
#define N_TOLERANCES (sizeof tolerances / sizeof tolerances[0])

static double mu;
static Cr3bpCtx ctx;

static Dop853Config config(double tol)
{
    Dop853Config cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.tol_m = tol;
    cfg.max_steps = 50000000;
    return cfg;
}

/* Relative loss of the Jacobi constant, which the true dynamics hold exactly -
 * so every digit here was lost by the numerics and by nothing else. */
static double drift(const State *start, const State *now)
{
    double c0 = cr3bp_jacobi(start->r, start->v, mu);
    double c1 = cr3bp_jacobi(now->r, now->v, mu);
    return fabs((c1 - c0) / c0);
}

static int export_jacobi(const Case *cases, int n_cases)
{
    Csv c;
    if (!csv_open(&c, "build/csv/jacobi.csv",
                  "orbit,tolerance,revolutions,drift,steps")) {
        return 0;
    }

    for (int i = 0; i < n_cases; i++) {
        for (size_t k = 0; k < N_TOLERANCES; k++) {
            Dop853Config cfg = config(tolerances[k]);
            Dop853State st;
            memset(&st, 0, sizeof st);

            State end;
            if (dop853_integrate(accel_cr3bp, &ctx, &cases[i].start,
                                 (double)REVOLUTIONS * TWO_PI, &cfg, &st,
                                 &end) != CORE_OK) {
                fprintf(stderr, "ex_accuracy: %s did not integrate at tol "
                                "%g\n", cases[i].name, tolerances[k]);
                return 0;
            }

            csv_named(&c, cases[i].name, 4, tolerances[k],
                      (double)REVOLUTIONS, drift(&cases[i].start, &end),
                      (double)st.n_accepted);
        }
    }

    return csv_close(&c);
}

static int export_jacobi_growth(const Case *cases, int n_cases)
{
    Csv c;
    if (!csv_open(&c, "build/csv/jacobi_growth.csv",
                  "orbit,revolution,drift")) {
        return 0;
    }

    for (int i = 0; i < n_cases; i++) {
        Dop853Config cfg = config(GROWTH_TOLERANCE);
        Dop853State st;
        memset(&st, 0, sizeof st);

        State s = cases[i].start;

        /* One integration stopped at every revolution, not a hundred
         * integrations - the step carries across the stops, so this is the
         * same trajectory the single-shot run above flies, sampled. */
        for (int rev = 0; rev <= REVOLUTIONS; rev++) {
            if (rev > 0) {
                State out;
                if (dop853_integrate(accel_cr3bp, &ctx, &s,
                                     (double)rev * TWO_PI, &cfg, &st, &out)
                    != CORE_OK) {
                    return 0;
                }
                s = out;
            }

            csv_named(&c, cases[i].name, 2, (double)rev,
                      drift(&cases[i].start, &s));
        }
    }

    return csv_close(&c);
}

static State circular(double radius)
{
    double v = sqrt(MU_EARTH / radius);
    State s = { { radius, 0.0, 0.0 }, { 0.0, v, 0.0 }, 0.0 };
    return s;
}

static int export_reversibility(void)
{
    Csv c;
    if (!csv_open(&c, "build/csv/reversibility.csv",
                  "years,revolutions,error_m,energy_drift")) {
        return 0;
    }

    TwoBodyCtx two_body = { MU_EARTH };
    State start = circular(R_LUNAR);
    double period = two_body_period(start.r, start.v, MU_EARTH);

    for (int year = 1; year <= MAX_YEARS; year++) {
        double span = (double)year * YEAR;

        Dop853Config cfg;
        memset(&cfg, 0, sizeof cfg);
        cfg.tol_m = 1e-6;
        cfg.max_steps = 2000000;

        Dop853State forward_st;
        memset(&forward_st, 0, sizeof forward_st);
        State forward;
        if (dop853_integrate(accel_two_body, &two_body, &start, span, &cfg,
                             &forward_st, &forward) != CORE_OK) {
            return 0;
        }

        /* A fresh Dop853State for the return leg, deliberately. Handing back
         * the forward one would let the step sequence retrace itself and the
         * errors cancel, which measures nothing. This is an independent
         * integration that happens to run the other way. */
        Dop853State back_st;
        memset(&back_st, 0, sizeof back_st);
        State back;
        if (dop853_integrate(accel_two_body, &two_body, &forward, 0.0, &cfg,
                             &back_st, &back) != CORE_OK) {
            return 0;
        }

        double e0 = two_body_energy(start.r, start.v, MU_EARTH);
        double e1 = two_body_energy(forward.r, forward.v, MU_EARTH);

        csv_row(&c, 4, (double)year, span / period,
                vec3_distance(back.r, start.r), fabs((e1 - e0) / e0));
    }

    return csv_close(&c);
}

int main(void)
{
    mu = cr3bp_mu(GM_EARTH, GM_MOON);
    ctx.mu = mu;

    Vec3d l4;
    if (cr3bp_lagrange(mu, 4, &l4) != CORE_OK) {
        return 1;
    }

    Case cases[2];
    cases[0].name = "l4_region";
    cases[0].start = (State){ { l4.x + 0.02, l4.y, 0.0 },
                              { 0.0, 0.0, 0.0 }, 0.0 };
    cases[1].name = "close_approach";
    cases[1].start = (State){ { 0.5, 0.0, 0.0 }, { 0.0, 0.6, 0.0 }, 0.0 };

    printf("ex_accuracy: %d revolutions, %zu tolerances\n",
           REVOLUTIONS, N_TOLERANCES);

    if (!export_jacobi(cases, 2)
        || !export_jacobi_growth(cases, 2)
        || !export_reversibility()) {
        return 1;
    }

    return 0;
}
