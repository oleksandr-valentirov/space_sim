/* Determinism scenario: the third-order halo approximation.
 *
 * Kept apart from sc_correct rather than folded into it, because it is the
 * only scenario that hashes a long chain of pure arithmetic with no
 * integration in it at all. If a platform ever disagrees, ROADMAP C5 says to
 * split the scenario until the first differing operation is found - and this
 * one is already the half that has no integrator, which makes it the first
 * place to look and the fastest to bisect.
 *
 * Prints one line: <name> <hash>. See core/scenario/golden.txt. */

#include "cr3bp.h"
#include "hash.h"
#include "richardson.h"

#include <stdio.h>

static double opaque(double x)
{
    volatile double v = x;
    return v;
}

int main(void)
{
    CoreHash h;
    core_hash_init(&h);

    /* Three mass ratios, not one: the coefficients depend on mu through a
     * bisection for the libration point and then through five nested
     * divisions, and a platform difference could easily show at one value and
     * not another. Earth-Moon, Sun-Earth, and a deliberately extreme ratio. */
    double ratios[3] = {
        opaque(1.215058560962404e-02),
        opaque(3.003480593992994e-06),
        opaque(0.3),
    };

    for (int m = 0; m < 3; m++) {
        double mu = ratios[m];

        for (int point = 1; point <= 2; point++) {
            /* Amplitudes are taken as fractions of gamma rather than as fixed
             * numbers, because the reach of the series scales with it. A step
             * of 0.008 in absolute units covers most of the Earth-Moon L2
             * family and lies entirely outside the Sun-Earth one, where gamma
             * is a hundredth instead of a sixth - the scenario would have
             * hashed nothing but error codes for two of the three ratios. */
            Vec3d li;
            if (cr3bp_lagrange(mu, point, &li) != CORE_OK) {
                return 1;
            }
            double gamma = point == 1 ? (1.0 - mu) - li.x : li.x - (1.0 - mu);
            core_hash_f64(&h, gamma);

            for (int step = -8; step <= 8; step++) {
                if (step == 0) {
                    continue;
                }

                double az = gamma * opaque(0.05) * (double)step;

                State s;
                double period;
                CoreResult r = richardson_halo(mu, point, az, &s, &period);

                /* Failures are hashed too. Where the amplitude constraint
                 * stops having a solution is itself a floating-point
                 * decision, and a platform that draws that line one step
                 * differently must not pass. */
                core_hash_f64(&h, (double)r);
                if (r != CORE_OK) {
                    continue;
                }

                core_hash_f64(&h, s.r.x);
                core_hash_f64(&h, s.r.y);
                core_hash_f64(&h, s.r.z);
                core_hash_f64(&h, s.v.x);
                core_hash_f64(&h, s.v.y);
                core_hash_f64(&h, s.v.z);
                core_hash_f64(&h, period);
            }
        }
    }

    printf("sc_richardson %016llx\n", (unsigned long long)core_hash_value(&h));
    return 0;
}
