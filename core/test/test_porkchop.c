/* Porkchop-plot grid (ROADMAP.md, M3, "Планування"). */

#include "ephemeris.h"
#include "lambert.h"
#include "porkchop.h"
#include "test.h"

#include <math.h>
#include <stdio.h>

#define MU_SUN   1.32712440018e20
#define R_EARTH  1.495978707e11 /* 1 AU */
#define R_MARS   2.279392e11    /* ~1.524 AU */

static const double TEST_PI = 3.14159265358979323846;

typedef struct {
    double radius;
    double omega; /* mean motion, sqrt(mu / r^3) */
} CircularOrbit;

static CircularOrbit circular_orbit(double radius, double mu)
{
    CircularOrbit o;
    o.radius = radius;
    o.omega = sqrt(mu / (radius * radius * radius));
    return o;
}

static CoreResult circular_state(double t, void *ctx_, Vec3d *r, Vec3d *v)
{
    const CircularOrbit *o = ctx_;
    double theta = o->omega * t;
    double c = cos(theta), s = sin(theta);
    double speed = o->radius * o->omega;
    *r = vec3(o->radius * c, o->radius * s, 0.0);
    *v = vec3(-speed * s, speed * c, 0.0);
    return CORE_OK;
}

/* Analytic two-impulse Hohmann transfer between the two circular orbits
 * above: the minimum-energy transfer between coplanar circular orbits when
 * free to pick the departure phase, so the grid search below can never beat
 * it - only approach it, as closely as its resolution allows. Vis-viva, the
 * same formula core/test/test_lambert.c's periapsis() helper already uses. */
static void hohmann(double r1, double r2, double mu,
                    double *v_inf_depart, double *v_inf_arrive, double *tof)
{
    double a = 0.5 * (r1 + r2);
    double v1_circ = sqrt(mu / r1);
    double v2_circ = sqrt(mu / r2);
    double v1_transfer = sqrt(mu * (2.0 / r1 - 1.0 / a));
    double v2_transfer = sqrt(mu * (2.0 / r2 - 1.0 / a));

    *v_inf_depart = v1_transfer - v1_circ;
    *v_inf_arrive = v2_circ - v2_transfer;
    *tof = TEST_PI * sqrt(a * a * a / mu);
}

int main(void)
{
    CircularOrbit earth = circular_orbit(R_EARTH, MU_SUN);
    CircularOrbit mars = circular_orbit(R_MARS, MU_SUN);

    double v_inf_d, v_inf_a, tof_hohmann;
    hohmann(R_EARTH, R_MARS, MU_SUN, &v_inf_d, &v_inf_a, &tof_hohmann);
    double hohmann_total = v_inf_d + v_inf_a;

    double t_earth = 2.0 * TEST_PI / earth.omega;
    double t_mars = 2.0 * TEST_PI / mars.omega;
    double synodic = 1.0 / fabs(1.0 / t_earth - 1.0 / t_mars);

    /* Departure grid spans more than one synodic period, so some cell is
     * always close to the phase Hohmann needs. Time-of-flight grid brackets
     * the Hohmann transfer time on both sides. */
    enum { N_T1 = 120, N_TOF = 60 };
    double t1_grid[N_T1];
    for (int i = 0; i < N_T1; i++) {
        t1_grid[i] = (1.1 * synodic) * (double)i / (double)(N_T1 - 1);
    }
    double tof_grid[N_TOF];
    for (int i = 0; i < N_TOF; i++) {
        double frac = 0.5 + (double)i / (double)(N_TOF - 1); /* 0.5 .. 1.5 */
        tof_grid[i] = frac * tof_hohmann;
    }

    static PorkchopPoint points[N_T1 * N_TOF];
    size_t count = 0;
    CoreResult r = porkchop_compute(circular_state, &earth, circular_state, &mars,
                                    MU_SUN, 1, t1_grid, N_T1, tof_grid, N_TOF,
                                    points, N_T1 * N_TOF, &count);
    CHECK(r == CORE_OK);
    CHECK(count > 0);

    double best = -1.0;
    for (size_t i = 0; i < count; i++) {
        double total = points[i].v_inf_depart + points[i].v_inf_arrive;
        CHECK(points[i].v_inf_depart >= 0.0);
        CHECK(points[i].v_inf_arrive >= 0.0);
        CHECK(points[i].tof > 0.0);
        if (best < 0.0 || total < best) {
            best = total;
        }
    }

    /* Hohmann is the minimum-energy transfer, so nothing in the grid can beat
     * it (up to floating-point slack); measured with this grid, the closest
     * cell lands 0.042% above it (5595.9 vs 5593.6 m/s) - every one of the
     * 120*60 cells converged for this coplanar, circular geometry, so that
     * gap is entirely the departure-time grid missing the exact Hohmann
     * phase by a fraction of a step, not a solver shortfall. */
    CHECK(best > hohmann_total * (1.0 - 1e-6));
    CHECK(best < hohmann_total * 1.001);

    /* Buffer too small: same grid, capacity for one point. count still comes
     * back set to what was actually written (core/refdata.h convention). */
    {
        PorkchopPoint one[1];
        size_t small_count = 0;
        CoreResult small_r = porkchop_compute(circular_state, &earth, circular_state,
                                              &mars, MU_SUN, 1, t1_grid, N_T1,
                                              tof_grid, N_TOF, one, 1, &small_count);
        CHECK(small_r == CORE_ERR_BUFFER_TOO_SMALL);
        CHECK(small_count == 1);
    }

    /* Invalid arguments, rejected before either BodyStateFunc is called. */
    {
        PorkchopPoint out[1];
        size_t n = 0;
        CHECK(porkchop_compute(NULL, &earth, circular_state, &mars, MU_SUN, 1,
                               t1_grid, N_T1, tof_grid, N_TOF, out, 1, &n)
              == CORE_ERR_INVALID_ARG);
        CHECK(porkchop_compute(circular_state, &earth, circular_state, &mars,
                               -1.0, 1, t1_grid, N_T1, tof_grid, N_TOF, out, 1, &n)
              == CORE_ERR_INVALID_ARG);
        CHECK(porkchop_compute(circular_state, &earth, circular_state, &mars,
                               MU_SUN, 1, t1_grid, 0, tof_grid, N_TOF, out, 1, &n)
              == CORE_ERR_INVALID_ARG);
    }

    /* The batch entry point that can cross into Rust (ROADMAP-UI.md U5a).
     *
     * The oracle is the one the step named: a cell of the plot must equal a
     * direct lambert_solve with the same arguments. Not the same code - the
     * same result. That is what catches swapped grid axes, and they do get
     * swapped: a grid of t1 by tof transposed still fills every cell with
     * plausible speeds. */
    {
        EphemerisCtx *eph = NULL;
        if (eph_load("data/fixture/earth_moon.eph", &eph) != CORE_OK) {
            fprintf(stderr, "test_porkchop: cannot read fixture; "
                            "run from the repository root\n");
            return 1;
        }

        const int EARTH_BODY = 3, MOON_BODY = 4;
        double mu = eph_body_mu(eph, EARTH_BODY);
        CHECK(mu > 0.0);

        double t1s[3] = { 0.0, 3.0 * 86400.0, 6.0 * 86400.0 };
        double tofs[2] = { 4.0 * 86400.0, 5.0 * 86400.0 };

        PorkchopPoint grid[6];
        size_t n = 0;
        CHECK(porkchop_compute_eph(eph, EARTH_BODY, MOON_BODY, mu, 1,
                                   t1s, 3, tofs, 2, grid, 6, &n) == CORE_OK);
        CHECK(n > 0);

        for (size_t i = 0; i < n; i++) {
            State from, to;
            CHECK(eph_body_state(eph, EARTH_BODY, grid[i].t1, &from) == CORE_OK);
            CHECK(eph_body_state(eph, MOON_BODY, grid[i].t1 + grid[i].tof, &to)
                  == CORE_OK);

            Vec3d v1, v2;
            CHECK(lambert_solve(from.r, to.r, grid[i].tof, mu, 1, 0, &v1, &v2)
                  == CORE_OK);

            double dvx = v1.x - from.v.x, dvy = v1.y - from.v.y, dvz = v1.z - from.v.z;
            double depart = sqrt(dvx * dvx + dvy * dvy + dvz * dvz);
            double ax = v2.x - to.v.x, ay = v2.y - to.v.y, az = v2.z - to.v.z;
            double arrive = sqrt(ax * ax + ay * ay + az * az);

            /* Bitwise would be overreach - the wrapper computes the same
             * quantities through the same functions, but the compiler is free
             * to keep an intermediate in a register on one path and not the
             * other. A part in 10^12 is far below anything a swapped axis or
             * a wrong body could survive. */
            CHECK(fabs(grid[i].v_inf_depart - depart) <= 1e-12 * depart);
            CHECK(fabs(grid[i].v_inf_arrive - arrive) <= 1e-12 * arrive);
        }

        /* And a NULL ephemeris is refused rather than dereferenced. */
        size_t none = 0;
        CHECK(porkchop_compute_eph(NULL, EARTH_BODY, MOON_BODY, mu, 1,
                                   t1s, 3, tofs, 2, grid, 6, &none)
              == CORE_ERR_INVALID_ARG);

        eph_free(eph);
    }

    return TEST_RESULT();
}
