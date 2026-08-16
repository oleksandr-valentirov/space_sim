/* Oracle for the planning FFI declarations (ROADMAP L3, debt D1).
 *
 * A second oracle rather than more tags in the first, for exactly one reason:
 * THIS ONE LINKS WITH `-lm`. `core-sys/oracle.c` links without it on purpose
 * -- linking there is the check that no trigonometry seeped into the runtime
 * zone. Adding `lambert_solve`, which calls `acos`, `sinh` and `cosh`, would
 * remove that check for convenience.
 *
 * The same claim at library level: `libcore_planning.a` is separate from
 * `libcore.a` (core-sys/build.rs), because the determinism boundary runs along
 * propagation, not planning (PROJECT.md §4).
 *
 * What the comparison checks: `lambert_solve` is the first boundary function
 * taking a struct **by value** rather than by pointer. A Vec3d of three double
 * fits in no register set under any of our ABIs, so it travels through memory,
 * and if Rust and C disagreed about that the result would not be a crash but
 * plausible velocities. Hence the bitwise comparison.
 *
 * Same format as oracle.c: first field is a tag, then numbers in %.17g.
 *
 *   lam  <v1x> <v1y> <v1z> <v2x> <v2y> <v2z>   successful solution
 *   lerr <code>                                 failure code
 *   pork <k> <t1> <tof> <v_inf_depart> <v_inf_arrive>   grid cell
 *
 * `pork` arrived with U5a: `porkchop_compute_eph` reads the ephemeris, so the
 * oracle now does read the asset -- and running from the repository root
 * became mandatory rather than cosmetic. */

#include "ephemeris.h"
#include "lambert.h"
#include "porkchop.h"

#include <stdio.h>

/* A heliocentric transfer, because that is what Lambert exists for in this
 * game (PROJECT.md §8, porkchop). The Sun's mu comes from data/horizons; the
 * radii and flight time are round numbers of the order of Earth's and Mars's
 * orbits rather than ephemeris values: the oracle checks the boundary, not
 * astronomy, and tying it to the asset would make it sensitive to `make cook`.
 *
 * The plane does not coincide with xy: the third component is non-zero at both
 * points. Same lesson K7b drew from the drag gradient -- a check placed where
 * a component is identically zero says nothing about a whole column. */
#define MU_SUN 1.32712440018e20

static const Vec3d R1 = { 1.4959787e11, 0.0, 0.0 };
static const Vec3d R2 = { -1.9e11, 1.1e11, 8.0e9 };

#define TOF_S (2.5e7) /* ~289 days, the order of a real Mars window */

#define ASSET "data/fixture/earth_moon.eph"
#define DAY 86400.0

/* An Earth-to-Moon grid: three departure dates, two flight times. Small on
 * purpose -- the oracle checks layout and field order, not astronomy. */
static void porkchop(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "oracle_planning: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root\n");
        return;
    }

    const int EARTH = 3, MOON = 4;
    double t1s[3] = { 0.0, 3.0 * DAY, 6.0 * DAY };
    double tofs[2] = { 4.0 * DAY, 5.0 * DAY };

    PorkchopPoint grid[6];
    size_t n = 0;
    if (porkchop_compute_eph(eph, EARTH, MOON, eph_body_mu(eph, EARTH), 1,
                             t1s, 3, tofs, 2, grid, 6, &n) != CORE_OK) {
        fprintf(stderr, "oracle_planning: the grid did not compute\n");
        eph_free(eph);
        return;
    }

    for (size_t k = 0; k < n; k++) {
        printf("pork %zu %.17g %.17g %.17g %.17g\n",
               k, grid[k].t1, grid[k].tof,
               grid[k].v_inf_depart, grid[k].v_inf_arrive);
    }

    eph_free(eph);
}

static void print_pair(const char *tag, Vec3d v1, Vec3d v2)
{
    printf("%s %.17g %.17g %.17g %.17g %.17g %.17g\n",
           tag, v1.x, v1.y, v1.z, v2.x, v2.y, v2.z);
}

int main(void)
{
    Vec3d v1, v2;

    /* Prograde branch and retrograde. Both, because `prograde` is the sign of
     * the z component of angular momentum, not "short or long arc" (ROADMAP,
     * "Фізика й пропагація"), and a swapped int here would give a perfectly
     * plausible solution to a different problem. */
    if (lambert_solve(R1, R2, TOF_S, MU_SUN, 1, 0, &v1, &v2) != CORE_OK) {
        fprintf(stderr, "oracle_planning: prograde transfer did not converge\n");
        return 1;
    }
    print_pair("lam", v1, v2);

    if (lambert_solve(R1, R2, TOF_S, MU_SUN, 0, 0, &v1, &v2) != CORE_OK) {
        fprintf(stderr, "oracle_planning: retrograde transfer did not converge\n");
        return 1;
    }
    print_pair("lam", v1, v2);

    /* And a rejection. The return code crosses the boundary too, and
     * `CoreResult` as a `c_int` with constants (rather than a Rust enum) only
     * makes sense if someone actually compares the values. n_revs != 0 is a
     * documented rejection in lambert.h. */
    printf("lerr %d\n", (int)lambert_solve(R1, R2, TOF_S, MU_SUN, 1, 1, &v1, &v2));

    /* A second rejection of a different origin: degenerate geometry. With r1
     * and r2 on one line through the origin the transfer plane is
     * undefined. */
    Vec3d opposite = { -R1.x, -R1.y, -R1.z };
    printf("lerr %d\n",
           (int)lambert_solve(R1, opposite, TOF_S, MU_SUN, 1, 0, &v1, &v2));

    porkchop();

    return 0;
}
