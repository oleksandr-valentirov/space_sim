/* Physics-core throughput benchmark: DOP853, the runtime integrator
 * (skill perf-probe).
 *
 * Not a determinism scenario. Wall-clock time is hardware-dependent by
 * definition, so this prints numbers to stdout, not a hash, and is never
 * compared to core/scenario/golden.txt. It exists to answer the physics
 * half of "what does a system need to run this": PROJECT.md section 6 warp
 * is "how many integrator steps per frame", and section 4's two-tier ship
 * physics eventually means many vessels propagated per tick. This measures
 * the one number both of those are built on - accel evals and accepted
 * steps per second, single-threaded (invariant 4, CLAUDE.md: physics never
 * uses rayon) - on whatever machine runs it.
 *
 * Same eccentric, inclined two-body orbit as core/scenario/sc_dop853.c:
 * periapsis works the step controller hardest, so this is not an easy case
 * dressed up as a benchmark.
 *
 * Links against libcore.a only, no -lm, the same rule scenario binaries
 * follow: if a stray trig call reached the runtime integrator, linking
 * would fail here too, before the number even gets measured. */

/* clock_gettime/CLOCK_MONOTONIC are POSIX, not C11 - glibc hides them under
 * -std=c11 without this, since strict C11 alone does not ask for them. Must
 * come before any header pulls in features.h and fixes the feature set. */
#define _POSIX_C_SOURCE 199309L

#include "accel.h"
#include "integrator.h"

#include <stdio.h>
#include <time.h>

/* Stop after this many seconds of wall time, whichever leg that falls in.
 * A fixed leg count would take a different amount of wall time on every
 * machine, which is backwards for a benchmark whose whole point is to
 * measure exactly that. */
#define BUDGET_S 2.0
#define MAX_LEGS 2000000L

static double now_s(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

int main(void)
{
    TwoBodyCtx ctx = { 3.98600435436e14 };

    State s = {
        { 7.0e6, 0.0, 0.0 },
        { 0.0, 9546.0, 1200.0 },
        0.0,
    };

    Dop853Config cfg = { 0 };
    cfg.tol_m = 1e-6;

    Dop853State st = { 0 };

    double start = now_s();
    double elapsed = 0.0;
    long legs = 0;

    while (legs < MAX_LEGS) {
        State next;
        double t_end = 400.0 * (double)(legs + 1);

        if (dop853_integrate(accel_two_body, &ctx, &s, t_end, &cfg, &st,
                             &next) != CORE_OK) {
            fprintf(stderr, "bench_dop853: integration failed at leg %ld\n",
                    legs);
            return 1;
        }
        s = next;
        legs++;

        elapsed = now_s() - start;
        if (elapsed >= BUDGET_S) {
            break;
        }
    }

    long steps = st.n_accepted + st.n_rejected;
    double us_per_step = elapsed * 1e6 / (double)st.n_accepted;

    printf("bench_dop853: %ld leg(s), %.3f s wall\n", legs, elapsed);
    printf("  accepted steps   %8ld  (%.0f/s)\n", st.n_accepted,
           (double)st.n_accepted / elapsed);
    printf("  rejected steps   %8ld  (%.1f%% of attempts)\n", st.n_rejected,
           100.0 * (double)st.n_rejected / (double)steps);
    printf("  accel evals      %8ld  (%.0f/s)\n", st.n_evals,
           (double)st.n_evals / elapsed);
    printf("  per accepted step: %.3f us, %.1f evals\n", us_per_step,
           (double)st.n_evals / (double)st.n_accepted);

    printf("\n  vessel-steps that fit a single-threaded physics tick at:\n");
    printf("    60 Hz (16.7 ms budget)  %.0f\n", 16667.0 / us_per_step);
    printf("    30 Hz (33.3 ms budget)  %.0f\n", 33333.0 / us_per_step);

    return 0;
}
