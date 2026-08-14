/* How much vessel-time one core buys (skill perf-probe).
 *
 * The third benchmark, and it exists because the other two together still do
 * not answer the question warp asks. bench_dop853 measures the integrator,
 * bench_field measures one force evaluation, and bench_field says outright
 * why it refuses to run DOP853 over accel_field: the step sizes of one
 * particular orbit would be measured along with the model, and that would
 * make a number about the force model dishonest.
 *
 * Those step sizes are exactly what this file is about. PROJECT.md section 4
 * defines warp as steps of simulation per frame, and how many steps a day of
 * simulation takes is a property of the ORBIT, not of the force model - a
 * geostationary vessel needs a fraction of what a low one does. So the two
 * questions are separated on purpose:
 *
 *     bench_field : nanoseconds per evaluation - the model's cost
 *     bench_prop  : accepted steps per simulated day - the orbit's demand
 *
 * and the product of the two, per regime, is the warp ceiling on one core.
 * Single core is the whole budget and not a share of it: physics never uses
 * rayon (CLAUDE.md, invariant 4), so this ceiling does not rise with cores.
 *
 * WHAT THE WARP FIGURE MEANS, AND WHAT IT DOES NOT. It is simulated seconds
 * per wall second for ONE vessel with the whole core to itself, at the
 * tolerance and step ceiling core-rs actually defaults to. Nine vessels
 * divide it, and it says nothing about whether the game can spend the memory
 * the resulting trajectory takes - the store keeps every accepted step
 * (game/src/leg.rs), so a high warp is also a high allocation rate, and that
 * is the limit that arrives first.
 *
 * WHY EACH REGIME RESTARTS. The fixture asset spans 120 days, and the
 * ceilings here are in the tens of millions - a run that simply flew on would
 * leave the span in milliseconds and spend the rest of the budget measuring
 * whatever the propagator does at the edge. So each repetition re-flies the
 * same span from the same state, and the arithmetic is identical every time.
 *
 * Same rules as its siblings: wall time is hardware-dependent by definition,
 * so this prints numbers rather than a hash and is never compared against
 * core/scenario/golden.txt. Links against libcore.a without -lm, which is
 * why the circular speeds below are written out rather than computed as
 * sqrt(mu/r). */

/* clock_gettime/CLOCK_MONOTONIC are POSIX, not C11 - see bench_dop853.c. */
#define _POSIX_C_SOURCE 199309L

#include "prop.h"

#include <stdio.h>
#include <string.h>
#include <time.h>

#define ASSET "data/fixture/earth_moon.eph"
#define EARTH 3
#define DAY 86400.0

/* Wall time per regime. Long enough that one propagation is not the whole
 * measurement, short enough that make bench stays interactive. */
#define BUDGET_S 0.3

/* The game's leg (game/src/world.rs), so the per-call overhead measured is
 * the one the game pays. */
#define CAP 256

/* Circular speeds for mu = 3.986004418e14 and the fixture's mean Earth radius
 * of 6371010 m, written out because this file links without -lm. Tilted out
 * of the equator (0.9205, 0.3907 is about 23 degrees) for the reason K7a and
 * K7b both found the hard way: a state with a zero component measures the
 * easiest case of every term that touches it. */
#define COS_I 0.92050
#define SIN_I 0.39073

typedef struct {
    const char *name;
    double altitude_m;
    double speed_ms;
    double span_s;    /* simulated time per repetition */
    int    with_drag;
} Regime;

/* Spans differ by regime because the interesting quantity is steps per
 * simulated day, and a day of geostationary flight is two dozen steps - too
 * few to time. All of them stay inside the asset's 120-day span. */
static const Regime REGIMES[] = {
    { "LEO 400 km, drag",    400.0e3,    7672.593,        DAY, 1 },
    { "LEO 400 km, no drag", 400.0e3,    7672.593,        DAY, 0 },
    { "MEO 2000 km",        2000.0e3,    6900.490,        DAY, 1 },
    { "GEO 35786 km",      35786.0e3,    3074.921, 10.0 * DAY, 1 },
    { "lunar distance",   384400.0e3,    1009.968, 30.0 * DAY, 1 },
};

static double now_s(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

static int run_regime(const Regime *regime, EphemerisCtx *eph, double t0,
                      const State *earth, const VesselParams *vessel_params)
{
    /* The pair core-rs defaults to (core-rs/src/lib.rs), not a tighter one
     * chosen to make the number look either way. */
    PropConfig cfg = { CORE_INTEG_DOP853, 1.0, 3600.0, 0, 1.0 };
    PropagatorCtx *p = NULL;
    State start;
    long steps = 0;
    long reps = 0;
    double wall = 0.0;
    double begin;

    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        fprintf(stderr, "bench_prop: prop_create failed for %s\n",
                regime->name);
        return 1;
    }

    /* Radially out along x from the Earth, moving in the y-z plane: circular
     * to the precision the printed speeds carry, which is all a cost
     * measurement needs from an orbit. */
    start.t = t0;
    start.r = vec3(earth->r.x + eph_body_radius(eph, EARTH)
                       + regime->altitude_m,
                   earth->r.y, earth->r.z);
    start.v = vec3(earth->v.x, earth->v.y + regime->speed_ms * COS_I,
                   earth->v.z + regime->speed_ms * SIN_I);

    begin = now_s();
    while (wall < BUDGET_S) {
        State vessel = start;
        double t_end = t0 + regime->span_s;
        double step = 0.0;

        for (;;) {
            State samples[CAP];
            size_t n = 0;
            State final_state;
            CoreStopReason stop;
            int event = -1;

            if (prop_run(p, &vessel, vessel_params, t_end, NULL, 0, samples,
                         CAP, &n, &final_state, &stop, &event, &step)
                != CORE_OK) {
                fprintf(stderr, "bench_prop: prop_run failed in %s\n",
                        regime->name);
                prop_free(p);
                return 1;
            }

            steps += (long)n;
            vessel = final_state;

            if (stop == CORE_STOP_T_END) {
                break;
            }
        }

        reps++;
        wall = now_s() - begin;
    }

    prop_free(p);

    {
        double sim_days = regime->span_s * (double)reps / DAY;
        double us_per_step = wall * 1e6 / (double)steps;
        double steps_per_day = (double)steps / sim_days;

        /* 104 bytes is one game/src/leg.rs Sample: the state plus the Earth
         * and Moon positions the renderer needs beside it. History is never
         * trimmed, so this is the rate at which a vessel at this warp
         * consumes memory, not a transient. */
        double mib_per_hour = steps_per_day * 104.0
                              * (regime->span_s * (double)reps / wall) * 3600.0
                              / DAY / (1024.0 * 1024.0);

        printf("  %-20s %6.2f us/step %7.1f steps/sim-day  warp x%-9.3g"
               " %8.1f MiB/h\n",
               regime->name, us_per_step, steps_per_day,
               regime->span_s * (double)reps / wall, mib_per_hour);
    }
    return 0;
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    double t_begin, t_span_end;
    double t0;
    State earth;
    size_t i;

    /* 1000 kg on 20 m^2 at cr 1.3, cd 2.2 - a smallsat's ballistic
     * coefficient. bench_field already showed what each term costs; here the
     * vessel exists so that every term is switched on. */
    VesselParams sat = { 1000.0, 20.0, 1.3, 2.2 };
    VesselParams sat_no_drag = sat;

    sat_no_drag.cd = 0.0;

    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "bench_prop: cannot load %s\n", ASSET);
        fprintf(stderr, "  run from the repository root; `make cook` builds it\n");
        return 1;
    }

    if (eph_span(eph, &t_begin, &t_span_end) != CORE_OK) {
        eph_free(eph);
        return 1;
    }

    /* A day in, so the first Chebyshev interval is not a special case. */
    t0 = t_begin + DAY;

    if (eph_body_state(eph, EARTH, t0, &earth) != CORE_OK) {
        fprintf(stderr, "bench_prop: cannot read the Earth\n");
        eph_free(eph);
        return 1;
    }

    printf("bench_prop: %s, %d bodies, %.1f s per regime\n", ASSET,
           eph_body_count(eph), BUDGET_S);
    printf("  tol_m 1.0, h_max_s 3600, leg %d samples (core-rs defaults);"
           " vessel 1000 kg, 20 m^2\n", CAP);
    printf("  MiB/h is retained history at that warp: 104 bytes a sample,"
           " never trimmed\n");

    for (i = 0; i < sizeof REGIMES / sizeof REGIMES[0]; i++) {
        const VesselParams *vp = REGIMES[i].with_drag ? &sat : &sat_no_drag;

        if (run_regime(&REGIMES[i], eph, t0, &earth, vp) != 0) {
            eph_free(eph);
            return 1;
        }
    }

    eph_free(eph);
    return 0;
}
