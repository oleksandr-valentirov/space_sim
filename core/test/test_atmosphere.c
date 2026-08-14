/* Atmospheric density and drag (ROADMAP K7a).
 *
 * Three oracles, because the module makes three separable claims and one
 * check would let them hide behind each other.
 *
 * 1. atmosphere_exp_neg against libm's exp. A pure question of numerics: how
 *    far is our series from the function it approximates. Tests may link libm;
 *    core/atmosphere.c may not, and `make check-libm` is what enforces that
 *    rather than this file.
 *
 * 2. The published table against ITSELF. Vallado's bands overlap in a way that
 *    is easy to miss and useful to exploit: each band's quoted base density is
 *    what the band below it predicts at that altitude. So the table has to be
 *    continuous at all 27 interior joins, and a single mistyped digit breaks
 *    that somewhere. This is the check that the data went in correctly, and it
 *    does not consist of quoting the same numbers a second time - which would
 *    have said nothing at all.
 *
 * 3. Both Jacobians against finite differences, SEPARATELY. Drag is the first
 *    force in the core with a nonzero d(a)/d(v), and a mistake confined to
 *    that block does not show up in a position sweep at all. The position
 *    check is built the way core/field.c will build it in K7b - a spherical
 *    body, an altitude, a local vertical - so that what is verified is the
 *    chain a caller actually forms. */

#include "atmosphere.h"
#include "test.h"

#include <math.h>

#define EARTH_R 6378137.0       /* data/horizons/obj_earth.txt, equatorial */

/* Cd * A / m for a spacecraft with a couple of square metres per tonne and a
 * blunt drag coefficient. Not a citation, and it does not need to be: every
 * check below is either linear in it or divides it out. */
#define COEFF 0.004

/* ---- 1. the series against libm ---------------------------------------- */

static void check_exp(void)
{
    double worst = 0.0;
    double worst_x = 0.0;

    /* Dense across the reduction's whole working range, so that the seams
     * between halvings are swept rather than stepped over. */
    for (int i = 0; i <= 640000; i++) {
        double x = (double)i * 1e-4;   /* 0 .. 64 */
        double ours = atmosphere_exp_neg(x);
        double ref = exp(-x);

        double err = fabs(ours - ref) / ref;
        if (err > worst) {
            worst = err;
            worst_x = x;
        }
    }

    printf("  exp_neg: max relative error %.3e at x = %.4f\n", worst, worst_x);
    CHECK(worst < 1e-13);

    /* The two ends, stated rather than swept. */
    CHECK_BITS_EQ(atmosphere_exp_neg(0.0), 1.0);
    CHECK_BITS_EQ(atmosphere_exp_neg(-1.0), 1.0);   /* clamped, not mirrored */
    CHECK_BITS_EQ(atmosphere_exp_neg(64.5), 0.0);

    /* Just inside the ceiling it is still a number, and a very small one. The
     * point of the constant is that the discontinuity there is invisible, so
     * the value at the edge is worth stating. */
    double edge = atmosphere_exp_neg(64.0);
    printf("  exp_neg at the cutoff: %.3e\n", edge);
    CHECK(edge > 0.0 && edge < 2e-28);
}

/* ---- 2. the table against itself ---------------------------------------- */

static void check_table_continuity(void)
{
    const AtmosphereModel *m = &ATMOSPHERE_EARTH_USSA76;
    double worst = 0.0;
    int worst_i = -1;
    int joins = 0;

    CHECK(m->n_layers == 28);

    for (int i = 1; i < m->n_layers; i++) {
        const AtmosphereLayer *below = &m->layer[i - 1];
        const AtmosphereLayer *here = &m->layer[i];

        /* Ascending, and strictly: two bands starting at the same altitude
         * would make the search below ambiguous. */
        CHECK(here->base_altitude_m > below->base_altitude_m);
        CHECK(here->base_density > 0.0);
        CHECK(here->scale_height_m > 0.0);

        /* What the band below predicts where the band above begins. */
        double dh = here->base_altitude_m - below->base_altitude_m;
        double predicted =
            below->base_density * exp(-dh / below->scale_height_m);

        double err = fabs(predicted - here->base_density) / here->base_density;

        /* The bottom band is the one loose join and it is the fit's doing,
         * not a typo: 0 to 25 km is a single exponential laid across the
         * troposphere AND the tropopause, where the real profile changes
         * slope, so its rho0 and H are a compromise that does not land on the
         * next band's base. Measured at 1.4e-3. Every other join is the
         * published table agreeing with itself to its own four figures, and
         * they are held an order tighter, because that is where a mistyped
         * digit would show. */
        if (i == 1) {
            CHECK(err < 3e-3);
        } else if (err > worst) {
            worst = err;
            worst_i = i;
        }
        joins++;
    }

    /* Counted, because a loop that quietly ran zero times passes every check
     * inside it - the trap ROADMAP K1 fell into and now tests against. */
    CHECK(joins == 27);

    printf("  table continuity: worst join above 25 km %.3e relative, "
           "band %d (%.0f km)\n",
           worst, worst_i,
           worst_i >= 0 ? m->layer[worst_i].base_altitude_m / 1000.0 : 0.0);

    /* WHAT THIS DOES AND DOES NOT CATCH, stated rather than implied. A wrong
     * exponent, a wrong leading digit or a transposed pair breaks continuity
     * by orders and dies here. A slip in the fourth significant figure moves a
     * join by about 2e-4 and survives - the published values are only given to
     * four figures, so no self-consistency check can see below that. This is
     * a guard against fat fingers, not a substitute for the source. */
    CHECK(worst < 5e-4);
}

/* ---- density itself ------------------------------------------------------ */

static void check_density(void)
{
    const AtmosphereModel *m = &ATMOSPHERE_EARTH_USSA76;

    /* Sea level, quoted straight from the table's first row. */
    double rho;
    atmosphere_density(m, 0.0, &rho, 0);
    CHECK(fabs(rho - 1.225) < 1e-12);

    /* Below the surface the value is held, not extrapolated. Without the
     * clamp this would be exp(+880) at the centre of the Earth. */
    double deep;
    atmosphere_density(m, -EARTH_R, &deep, 0);
    CHECK_BITS_EQ(deep, rho);

    /* Strictly decreasing, everywhere, in one-kilometre steps to well past
     * the top band. Monotonicity is what makes an altitude event usable: a
     * density that rose anywhere would give a vessel two altitudes with the
     * same drag and a root finder two answers. */
    double prev = rho;
    int steps = 0;
    for (double h = 1000.0; h <= 3.0e6; h += 1000.0) {
        double d;
        atmosphere_density(m, h, &d, 0);
        CHECK(d > 0.0);
        CHECK(d < prev);
        prev = d;
        steps++;
    }
    CHECK(steps == 3000);

    /* The derivative agrees with a finite difference of the density, which is
     * what ties -rho/H to the profile rather than to a formula written twice.
     *
     * Every probe sits WELL INSIDE a band, and that is not fastidiousness.
     * The model is genuinely discontinuous at a join - by up to a part in a
     * thousand, which check 2 above measures - while the difference this
     * check forms is a few parts in a hundred thousand of the density. A
     * centred difference straddling a join therefore measures the step, not
     * the slope, and the first version of this test did exactly that and read
     * 0.46. The joins are check 2's business; the slope is this one's. */
    const double probes[] = { 5.0e3, 95.5e3, 105.0e3, 270.0e3,
                              420.0e3, 550.0e3, 850.0e3 };
    double worst = 0.0;
    for (size_t i = 0; i < sizeof(probes) / sizeof(probes[0]); i++) {
        double h = probes[i];
        double step = 1.0;      /* metres; well inside any band */
        double lo, hi, analytic;
        atmosphere_density(m, h - step, &lo, 0);
        atmosphere_density(m, h + step, &hi, 0);
        atmosphere_density(m, h, 0, &analytic);

        double numeric = (hi - lo) / (2.0 * step);
        double err = fabs(numeric - analytic) / fabs(analytic);
        if (err > worst) {
            worst = err;
        }
    }
    printf("  d(rho)/dh vs finite differences: %.3e relative\n", worst);
    CHECK(worst < 1e-6);

    /* A model with no layers is vacuum, which is the state every body but one
     * is in and the reason drag costs nothing where there is none. */
    AtmosphereModel empty;
    empty.n_layers = 0;
    double vac = -1.0, dvac = -1.0;
    atmosphere_density(&empty, 300.0e3, &vac, &dvac);
    CHECK_BITS_EQ(vac, 0.0);
    CHECK_BITS_EQ(dvac, 0.0);
}

/* ---- the force ----------------------------------------------------------- */

static void check_accel(void)
{
    const AtmosphereModel *m = &ATMOSPHERE_EARTH_USSA76;

    /* Against the closed form, written out here rather than called. */
    double rho;
    atmosphere_density(m, 400.0e3, &rho, 0);

    Vec3d v = vec3(7000.0, -1200.0, 350.0);
    Vec3d a;
    drag_accel(rho, COEFF, v, &a);

    double speed = sqrt(vec3_dot(v, v));
    double k = -0.5 * rho * COEFF * speed;
    CHECK(fabs(a.x - k * v.x) < 1e-24);
    CHECK(fabs(a.y - k * v.y) < 1e-24);
    CHECK(fabs(a.z - k * v.z) < 1e-24);

    /* Opposes the motion exactly: antiparallel, no component across it. The
     * cross product is the statement that survives any scaling mistake. */
    Vec3d cross = vec3_cross(a, v);
    CHECK(fabs(cross.x) < 1e-20);
    CHECK(fabs(cross.y) < 1e-20);
    CHECK(fabs(cross.z) < 1e-20);
    CHECK(vec3_dot(a, v) < 0.0);

    printf("  a_drag at 400 km, cd*A/m = %.3g, 7.1 km/s: %.6e m/s^2\n",
           COEFF, sqrt(vec3_dot(a, a)));

    /* Quadratic in speed: doubling the velocity multiplies the acceleration
     * by four. This is the property RKN could not have carried (PROJECT.md
     * section 4) and the one a stray |v| would break. */
    Vec3d a2;
    drag_accel(rho, COEFF, vec3_scale(v, 2.0), &a2);
    double ratio = sqrt(vec3_dot(a2, a2)) / sqrt(vec3_dot(a, a));
    CHECK(fabs(ratio - 4.0) < 1e-12);

    /* No air, no vessel, or no motion: exactly zero, so a caller that never
     * asks for drag gets bit-for-bit what it got before K7. */
    Vec3d z;
    drag_accel(0.0, COEFF, v, &z);
    CHECK(vec3_equal_bits(z, vec3_zero()));
    drag_accel(rho, 0.0, v, &z);
    CHECK(vec3_equal_bits(z, vec3_zero()));
    drag_accel(rho, COEFF, vec3_zero(), &z);
    CHECK(vec3_equal_bits(z, vec3_zero()));
}

/* ---- the two Jacobians, separately --------------------------------------- */

/* The chain core/field.c will form in K7b: a position over a spherical body
 * becomes an altitude, an altitude becomes a density, and the local vertical
 * is the direction that altitude increases in. No wind here - the Jacobians
 * this file returns do not carry it, and mixing it in would test something
 * else. */
static Vec3d accel_at(Vec3d r, Vec3d v)
{
    double d = sqrt(vec3_dot(r, r));
    double rho;
    atmosphere_density(&ATMOSPHERE_EARTH_USSA76, d - EARTH_R, &rho, 0);

    Vec3d a;
    drag_accel(rho, COEFF, v, &a);
    return a;
}

static double component(Vec3d a, int i)
{
    return i == 0 ? a.x : (i == 1 ? a.y : a.z);
}

static Vec3d nudge(Vec3d p, int i, double h)
{
    if (i == 0) { p.x += h; }
    else if (i == 1) { p.y += h; }
    else { p.z += h; }
    return p;
}

static void check_jacobians(void)
{
    /* A low orbit, deliberately inclined so that no component of either
     * vector is zero: a Jacobian with two indices swapped survives a check
     * done on the equator and dies here.
     *
     * 305 km rather than 300, because 300 km is a band base. Sitting on one
     * puts the reference density and the nudged ones in DIFFERENT bands, and
     * the first version of this test did that and read 6e-2 - not a wrong
     * Jacobian, a discontinuous function differenced across its own step. */
    Vec3d r = vec3(0.62 * EARTH_R, 0.55 * EARTH_R, 0.42 * EARTH_R);
    r = vec3_scale(r, (EARTH_R + 305.0e3) / sqrt(vec3_dot(r, r)));

    /* From the length that came back, not from the length that was asked for.
     * They differ in the last bits, and the band search reads an altitude. */
    double d = sqrt(vec3_dot(r, r));

    Vec3d v = vec3(-3100.0, 6900.0, 1400.0);
    Vec3d up = vec3_scale(r, 1.0 / d);

    double rho, drho;
    atmosphere_density(&ATMOSPHERE_EARTH_USSA76, d - EARTH_R, &rho, &drho);

    double dadr[9], dadv[9];
    drag_jacobian(rho, drho, COEFF, v, up, dadr, dadv);

    /* Position. The step has to be small against the scale height (53 km at
     * 300 km) and large against the metre-scale rounding of |r|; ten metres
     * is four orders inside both. */
    double worst_r = 0.0;
    for (int j = 0; j < 3; j++) {
        double h = 10.0;
        Vec3d lo = accel_at(nudge(r, j, -h), v);
        Vec3d hi = accel_at(nudge(r, j, +h), v);
        for (int i = 0; i < 3; i++) {
            double numeric = (component(hi, i) - component(lo, i)) / (2.0 * h);
            double err = fabs(numeric - dadr[i * 3 + j]) /
                         fabs(dadr[i * 3 + j]);
            if (err > worst_r) {
                worst_r = err;
            }
        }
    }
    printf("  d(a)/d(r) vs finite differences: %.3e relative\n", worst_r);
    CHECK(worst_r < 1e-6);

    /* Velocity. This block is new to the core - every force before drag had
     * none - so it is checked on its own rather than folded into the sweep
     * above, where an error in it would not appear. */
    double worst_v = 0.0;
    for (int j = 0; j < 3; j++) {
        double h = 1.0;
        Vec3d lo = accel_at(r, nudge(v, j, -h));
        Vec3d hi = accel_at(r, nudge(v, j, +h));
        for (int i = 0; i < 3; i++) {
            double numeric = (component(hi, i) - component(lo, i)) / (2.0 * h);
            double err = fabs(numeric - dadv[i * 3 + j]) /
                         fabs(dadv[i * 3 + j]);
            if (err > worst_v) {
                worst_v = err;
            }
        }
    }
    printf("  d(a)/d(v) vs finite differences: %.3e relative\n", worst_v);
    CHECK(worst_v < 1e-8);

    /* The two symmetries, and they differ. d(a)/d(v) is symmetric to the bit,
     * because it is written mirrored. d(a)/d(r) is rank one and must NOT be:
     * asserting that it is asymmetric is what stops a future tidy-up from
     * mirroring it "for consistency" with every other gradient in the core. */
    CHECK_BITS_EQ(dadv[1], dadv[3]);
    CHECK_BITS_EQ(dadv[2], dadv[6]);
    CHECK_BITS_EQ(dadv[5], dadv[7]);

    double asym = fabs(dadr[1] - dadr[3]) + fabs(dadr[2] - dadr[6]) +
                  fabs(dadr[5] - dadr[7]);
    double scale = fabs(dadr[1]) + fabs(dadr[3]) + fabs(dadr[2]) +
                   fabs(dadr[6]) + fabs(dadr[5]) + fabs(dadr[7]);
    printf("  d(a)/d(r) asymmetry: %.3e of its own scale\n", asym / scale);
    CHECK(asym > 0.1 * scale);

    /* Rank one: every 2x2 minor of the outer product vanishes. A second,
     * cheaper statement of the same structural fact, and one that would fail
     * loudly if a stray isotropic term ever crept in. */
    double minor = dadr[0] * dadr[4] - dadr[1] * dadr[3];
    CHECK(fabs(minor) < 1e-12 * (fabs(dadr[0] * dadr[4]) +
                                 fabs(dadr[1] * dadr[3])));

    /* No vessel means no matrix at all, both of them, bit for bit. */
    double zr[9], zv[9];
    drag_jacobian(rho, drho, 0.0, v, up, zr, zv);
    for (int k = 0; k < 9; k++) {
        CHECK_BITS_EQ(zr[k], 0.0);
        CHECK_BITS_EQ(zv[k], 0.0);
    }
    drag_jacobian(rho, drho, COEFF, vec3_zero(), up, zr, zv);
    for (int k = 0; k < 9; k++) {
        CHECK_BITS_EQ(zr[k], 0.0);
        CHECK_BITS_EQ(zv[k], 0.0);
    }
}

int main(void)
{
    printf("test_atmosphere\n");
    check_exp();
    check_table_continuity();
    check_density();
    check_accel();
    check_jacobians();
    return TEST_RESULT();
}
