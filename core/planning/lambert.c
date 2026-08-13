#include "lambert.h"

#include <math.h>

/* Not M_PI: that is POSIX, and the core builds with -std=c11 where it is not
 * guaranteed to exist (same reasoning as core/offline/cheb_fit.c). */
static const double LAMBERT_PI = 3.14159265358979323846;

/* Upper bound of the zero-revolution branch.
 *
 * C(z) = (1 - cos(sqrt(z))) / z is >= 0 for every z > 0 - the numerator
 * never goes negative - so it does NOT mark the edge of the zero-rev branch
 * the way a first reading of the Curtis/Vallado formulas suggests. What
 * actually happens at z = 4*pi^2 is that the transfer time F(z) diverges to
 * +infinity (the orbit period does, at the parabolic-to-multi-revolution
 * limit), and past it lies the low-z end of the ONE-revolution branch: a
 * different root of the same-looking equation, for a physically different
 * trajectory that happens to connect the same two points in the same time.
 * A search that does not stop here can wander across branches and never
 * settle - which is exactly what an unguarded Newton iteration from z = 0
 * did during testing (core/test/test_lambert.c, "circular long way": logged
 * steps of 60+ in z, straight past this boundary, before this constant and
 * the bracketed solver below replaced it). */
static const double LAMBERT_Z_HI = 39.478417; /* just under 4*pi^2 = 39.4784176... */

#define LAMBERT_MAX_ITER 100
#define LAMBERT_BRACKET_ITER 200

/* Convergence on z itself, not on F(z): F is scaled by sqrt(mu)*dt, which
 * ranges over many orders of magnitude between a low orbit transfer and an
 * interplanetary one, so a fixed tolerance on F would mean something
 * different in each case. z is dimensionless and O(1) to O(4*pi^2) for every
 * problem this solves, so a fixed step tolerance on it is comparable across
 * scales. */
#define LAMBERT_Z_TOL 1e-10

/* Stumpff functions C(z) and S(z), valid for every real z. Near z = 0 the
 * closed forms below are 0/0, so a Taylor expansion takes over there; it
 * converges fast enough that a few terms are exact to machine precision well
 * outside the switchover band. */
static void stumpff_cs(double z, double *c, double *s)
{
    if (z > 1e-6) {
        double sq = sqrt(z);
        *c = (1.0 - cos(sq)) / z;
        *s = (sq - sin(sq)) / (sq * sq * sq);
    } else if (z < -1e-6) {
        double sq = sqrt(-z);
        *c = (1.0 - cosh(sq)) / z;
        *s = (sinh(sq) - sq) / (sq * sq * sq);
    } else {
        *c = 0.5 - z / 24.0 + z * z / 720.0;
        *s = 1.0 / 6.0 - z / 120.0 + z * z / 5040.0;
    }
}

/* C(z), S(z) and their z-derivatives together, for the one place that needs
 * the derivatives (f_and_df).
 *
 * dC/dz = (1 - z*S - 2*C) / (2*z), dS/dz = (C - 3*S) / (2*z): the standard
 * recurrence (e.g. Vallado, "Fundamentals of Astrodynamics and
 * Applications"), checked here against the Taylor series it must agree with
 * as z -> 0 rather than trusted from memory alone. */
static void stumpff_all(double z, double *c, double *s, double *dc, double *ds)
{
    stumpff_cs(z, c, s);
    if (z > 1e-6 || z < -1e-6) {
        *dc = (1.0 - z * (*s) - 2.0 * (*c)) / (2.0 * z);
        *ds = ((*c) - 3.0 * (*s)) / (2.0 * z);
    } else {
        /* Fourth order is far past what a 1e-6-wide switchover band needs;
         * it exists so the switch itself cannot be seen in a plot of C', S'
         * against z. */
        *dc = -1.0 / 24.0 + z / 360.0;
        *ds = -1.0 / 120.0 + z / 2520.0;
    }
}

/* The transfer-orbit geometry that y(z) and F(z) close over. r1, r2 are the
 * chord endpoints' magnitudes; a bundles the direction-dependent factor that
 * is the same at every z. */
typedef struct {
    double r1, r2;
    double a;
    double sqrt_mu_dt;
} LambertGeometry;

static double y_of(const LambertGeometry *g, double z, double c, double s)
{
    return g->r1 + g->r2 + g->a * (z * s - 1.0) / sqrt(c);
}

/* Time-of-flight residual F(z) = sqrt(mu)*t(z) - sqrt(mu)*dt, and its
 * derivative dF/dz, evaluated together since they share every intermediate
 * quantity.
 *
 * F(z) = (y/C)^{3/2} * S + a*sqrt(y) - sqrt(mu)*dt        (Curtis eq. 5.32)
 *
 * dF/dz follows by the product and chain rule from y(z), C(z), S(z) and
 * their derivatives; nothing here is copied from a remembered closed form
 * for dF/dz itself; core/test/test_lambert.c checks the result of using it
 * against ground truth from the already-proven DOP853 integrator, which is
 * a check on the whole solve, not on this derivative in isolation. */
static void f_and_df(const LambertGeometry *g, double z, double *f, double *df)
{
    double c, s, dc, ds;
    stumpff_all(z, &c, &s, &dc, &ds);

    double y = y_of(g, z, c, s);
    double sqrt_c = sqrt(c);
    double sqrt_y = sqrt(y);

    /* dy/dz: y = r1 + r2 + a*w*q, w = z*s - 1, q = 1/sqrt(c). */
    double w = z * s - 1.0;
    double dw = s + z * ds;
    double q = 1.0 / sqrt_c;
    double dq = -0.5 * dc / (c * sqrt_c);
    double dy = g->a * (dw * q + w * dq);

    double u = y / c;
    double du = (dy * c - y * dc) / (c * c);
    double sqrt_u = sqrt(u);

    *f = u * sqrt_u * s + g->a * sqrt_y - g->sqrt_mu_dt;
    *df = 1.5 * sqrt_u * du * s + u * sqrt_u * ds + g->a * dy / (2.0 * sqrt_y);
}

typedef struct {
    int valid; /* z < LAMBERT_Z_HI and y(z) > 0: a legitimate zero-rev point */
    double f, df;
} LambertEval;

static LambertEval eval_z(const LambertGeometry *g, double z)
{
    LambertEval e;
    if (z >= LAMBERT_Z_HI) {
        e.valid = 0;
        e.f = e.df = 0.0;
        return e;
    }
    double c, s;
    stumpff_cs(z, &c, &s);
    if (!(y_of(g, z, c, s) > 0.0)) {
        e.valid = 0;
        e.f = e.df = 0.0;
        return e;
    }
    e.valid = 1;
    f_and_df(g, z, &e.f, &e.df);
    return e;
}

/* Bracket the root of F on the zero-rev branch: [*z_lo_out, *z_hi_out] with
 * F(z_lo) <= 0 <= F(z_hi), both points valid (eval_z.valid).
 *
 * F is monotonically increasing throughout (-infinity, LAMBERT_Z_HI) - the
 * scan in the LAMBERT_Z_HI comment above confirms this for the case that
 * motivated it - so expanding away from z = 0 in whichever direction F(0)
 * points can only ever find one sign change, never step past it. Doubling
 * the stride is what makes this cheap (a handful of evaluations for the
 * common case, near LAMBERT_BRACKET_ITER only for geometries that turn out
 * to have no zero-rev solution at all, which is exactly when this returns
 * 0 rather than looping forever). */
static int find_bracket(const LambertGeometry *g, double *z_lo_out, double *z_hi_out)
{
    LambertEval e0 = eval_z(g, 0.0);
    if (!e0.valid) {
        return 0;
    }
    if (e0.f == 0.0) {
        *z_lo_out = *z_hi_out = 0.0;
        return 1;
    }

    double known_z = 0.0, known_f = e0.f;
    double step = 8.0;
    for (int i = 0; i < LAMBERT_BRACKET_ITER; i++) {
        double candidate = (known_f < 0.0) ? known_z + step : known_z - step;
        if (candidate >= LAMBERT_Z_HI) {
            candidate = LAMBERT_Z_HI - (LAMBERT_Z_HI - known_z) * 0.5;
        }

        LambertEval e = eval_z(g, candidate);
        if (!e.valid || candidate == known_z) {
            step *= 0.5;
            if (step < 1e-9) {
                return 0; /* no zero-rev solution reaches this dt */
            }
            continue;
        }

        int sign_changed = (known_f < 0.0) ? (e.f >= 0.0) : (e.f <= 0.0);
        if (sign_changed) {
            if (known_f < 0.0) {
                *z_lo_out = known_z;
                *z_hi_out = candidate;
            } else {
                *z_lo_out = candidate;
                *z_hi_out = known_z;
            }
            return 1;
        }

        known_z = candidate;
        known_f = e.f;
        step *= 2.0;
    }
    return 0;
}

CoreResult lambert_solve(Vec3d r1, Vec3d r2, double dt, double mu,
                         int prograde, int n_revs,
                         Vec3d *v1_out, Vec3d *v2_out)
{
    if (n_revs != 0 || !(dt > 0.0) || !(mu > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    double r1n = vec3_norm(r1);
    double r2n = vec3_norm(r2);
    if (!(r1n > 0.0) || !(r2n > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    Vec3d cross12 = vec3_cross(r1, r2);
    double cross_norm = vec3_norm(cross12);
    if (!(cross_norm > 0.0)) {
        /* r1, r2 collinear (including antiparallel): the transfer plane, and
         * with it the prograde/retrograde convention, is undefined. */
        return CORE_ERR_INVALID_ARG;
    }

    double cos_theta = vec3_dot(r1, r2) / (r1n * r2n);
    if (cos_theta > 1.0) cos_theta = 1.0;
    if (cos_theta < -1.0) cos_theta = -1.0;
    double theta = acos(cos_theta); /* direction-agnostic, in (0, pi) */

    int cross_toward_plus_z = cross12.z >= 0.0;
    double dtheta = ((prograde != 0) == cross_toward_plus_z)
                        ? theta
                        : 2.0 * LAMBERT_PI - theta;

    LambertGeometry g;
    g.r1 = r1n;
    g.r2 = r2n;
    g.a = sin(dtheta) * sqrt(r1n * r2n / (1.0 - cos(dtheta)));
    g.sqrt_mu_dt = sqrt(mu) * dt;

    double z_lo, z_hi;
    if (!find_bracket(&g, &z_lo, &z_hi)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    /* Safeguarded Newton (Newton's method inside a shrinking bracket,
     * bisecting whenever it would step outside): plain Newton from z = 0
     * converges in a handful of iterations for most geometries, but for some
     * (see the LAMBERT_Z_HI comment) its very first step overshoots by tens
     * of units of z, straight past LAMBERT_Z_HI into a different branch's
     * root. Clamping to the bracket found above makes that impossible - the
     * iterate can never leave the interval that find_bracket already proved
     * contains exactly the zero-rev root - while still taking Newton's step
     * whenever it lands inside, so the fast cases stay fast. Same pattern as
     * core/correct.c's root search. */
    double z = 0.5 * (z_lo + z_hi);
    int converged = 0;
    for (int iter = 0; iter < LAMBERT_MAX_ITER; iter++) {
        LambertEval e = eval_z(&g, z);
        if (!e.valid) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        if (e.f < 0.0) {
            z_lo = z;
        } else {
            z_hi = z;
        }

        double next = (e.df != 0.0) ? z - e.f / e.df : 0.5 * (z_lo + z_hi);
        if (!(next > z_lo) || !(next < z_hi)) {
            next = 0.5 * (z_lo + z_hi);
        }

        double step = next - z;
        z = next;

        if (fabs(step) < LAMBERT_Z_TOL) {
            converged = 1;
            break;
        }
    }
    if (!converged) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    double c, s;
    stumpff_cs(z, &c, &s);
    double y = y_of(&g, z, c, s);
    if (!(y > 0.0)) {
        return CORE_ERR_TOLERANCE_NOT_MET;
    }

    double f_coef = 1.0 - y / g.r1;
    double gf_coef = g.a * sqrt(y / mu);
    double gdot_coef = 1.0 - y / g.r2;

    /* gf_coef is bounded away from 0: it is a*sqrt(y/mu), a is nonzero
     * because dtheta is strictly between 0 and 2*pi (the collinear check
     * above excludes both endpoints), and y > 0 was just checked. */
    Vec3d v1 = vec3_scale(vec3_sub(r2, vec3_scale(r1, f_coef)), 1.0 / gf_coef);
    Vec3d v2 = vec3_scale(vec3_sub(vec3_scale(r2, gdot_coef), r1), 1.0 / gf_coef);

    *v1_out = v1;
    *v2_out = v2;
    return CORE_OK;
}
