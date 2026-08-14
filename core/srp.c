#include "srp.h"

/* Written out rather than taken from math.h: M_PI is not in C11 proper, it is
 * a POSIX extension, and including math.h here would be the first step toward
 * something in this file calling into libm. */
#define SRP_PI 3.14159265358979323846

/* Abramowitz and Stegun 4.4.46. The quoted bound is 2e-8; sweeping four
 * million points against libm gives 2.179e-8 at x = 0, which is where the
 * leading coefficient's own truncation shows (1.5707963050 against pi/2 =
 * 1.5707963268). The test keeps that number honest. */
static const double ACOS_COEFF[8] = {
     1.5707963050, -0.2145988016,  0.0889789874, -0.0501743046,
     0.0308918810, -0.0170881256,  0.0066700901, -0.0012624911,
};

double srp_acos(double x)
{
    /* Clamped rather than trusted. Every caller here builds x as a ratio of
     * lengths that is mathematically in [-1, 1] but arrives through a
     * subtraction of comparable numbers, so 1 + 2e-16 is a real possibility -
     * and sqrt of a negative would turn a grazing eclipse into NaN. */
    if (x < -1.0) {
        x = -1.0;
    }
    if (x > 1.0) {
        x = 1.0;
    }

    int    negative = x < 0.0;
    double a = negative ? -x : x;

    /* Horner, in this order. The polynomial's evaluation order is part of the
     * result's last bits (PROJECT.md section 4), so it is written once here
     * and not rearranged. */
    double p = ACOS_COEFF[7];
    for (int i = 6; i >= 0; i--) {
        p = ACOS_COEFF[i] + a * p;
    }

    double v = sqrt(1.0 - a) * p;
    return negative ? SRP_PI - v : v;
}

double srp_shadow(Vec3d to_sun, double sun_radius,
                  Vec3d to_body, double body_radius)
{
    /* A body with no radius in the asset is not an occulter. This is the
     * common case for everything but the handful of bodies whose radius is
     * cited, and returning "fully lit" for them is the honest answer: we do
     * not know how big they are, and inventing a size would put a shadow
     * somewhere no data supports. */
    if (!(body_radius > 0.0) || !(sun_radius > 0.0)) {
        return 1.0;
    }

    /* The body is behind us, or beside us: no part of it is between the
     * vessel and the Sun. This is the branch nearly every evaluation takes,
     * which is why it comes before any square root. */
    double dot = vec3_dot(to_sun, to_body);
    if (!(dot > 0.0)) {
        return 1.0;
    }

    double ds2 = vec3_norm_sq(to_sun);
    double db2 = vec3_norm_sq(to_body);
    if (!(ds2 > 0.0) || !(db2 > 0.0)) {
        return 1.0;
    }

    double ds = sqrt(ds2);
    double db = sqrt(db2);

    /* Angular radii, through their sines: for a sphere of radius R seen from
     * distance d the tangent lines make sin(theta) = R/d exactly. No small
     * angle assumed yet - that comes later, in the planar overlap area. */
    double sin_b = body_radius / db;
    if (sin_b >= 1.0) {
        return 0.0;   /* inside the body */
    }
    double sin_a = sun_radius / ds;
    if (sin_a >= 1.0) {
        return 1.0;   /* inside the Sun; not a situation to model */
    }

    double cos_a = sqrt(1.0 - sin_a * sin_a);
    double cos_b = sqrt(1.0 - sin_b * sin_b);

    double cos_c = dot / (ds * db);
    if (cos_c > 1.0) {
        cos_c = 1.0;
    }

    /* Discs disjoint, i.e. separation >= a + b. Tested on cosines so the
     * fully-lit case, which is most of every orbit, never reaches srp_acos:
     * cos(a+b) = cos a cos b - sin a sin b, and the comparison flips because
     * cosine decreases. Both angles are at most pi/2, so a + b never passes
     * pi and the equivalence holds. */
    if (cos_c <= cos_a * cos_b - sin_a * sin_b) {
        return 1.0;
    }

    /* Only now, and only for a vessel actually in some kind of shadow. */
    double a = srp_acos(cos_a);
    double b = srp_acos(cos_b);
    double c = srp_acos(cos_c);

    if (c <= b - a) {
        return 0.0;                     /* umbra: the Sun is entirely behind */
    }
    if (c <= a - b) {
        double k = b / a;               /* annular: the body sits inside the disc */
        return 1.0 - k * k;
    }

    /* Partial: the lens-shaped overlap of two circles of radii a and b whose
     * centres are c apart. */
    double a2 = a * a;
    double b2 = b * b;
    double c2 = c * c;

    /* Clamped for the same reason srp_acos clamps, and here it is not a
     * remote possibility: when the two discs are nearly the same size and
     * nearly concentric, both numerators are differences of almost equal
     * squares divided by an almost-zero c. */
    double x1 = (c2 + a2 - b2) / (2.0 * c * a);
    double x2 = (c2 + b2 - a2) / (2.0 * c * b);

    double q = (-c + a + b) * (c + a - b) * (c - a + b) * (c + a + b);
    if (q < 0.0) {
        q = 0.0;
    }

    double area = a2 * srp_acos(x1) + b2 * srp_acos(x2) - 0.5 * sqrt(q);
    double f = 1.0 - area / (SRP_PI * a2);

    /* The approximation's error can push the fraction a hair outside [0, 1]
     * at the boundaries of this branch. A negative shadow would be a vessel
     * pulled toward the Sun. */
    if (f < 0.0) {
        f = 0.0;
    }
    if (f > 1.0) {
        f = 1.0;
    }
    return f;
}

void srp_accel(const SrpParams *p, Vec3d to_sun, double shadow, Vec3d *a_out)
{
    double d2 = vec3_norm_sq(to_sun);
    if (!(d2 > 0.0) || !(shadow > 0.0) || !(p->coeff > 0.0)) {
        *a_out = vec3_zero();
        return;
    }

    double d = sqrt(d2);

    /* (AU/d) squared as a product of two divisions rather than AU*AU/d2: at
     * an astronomical unit AU*AU is 2.2e22, which is fine, but at Neptune's
     * distance d2 is 2e25 and the ratio of the two loses digits the paired
     * form keeps. Cheap insurance in a term evaluated once per step. */
    double ratio = SRP_AU_M / d;
    double pressure = p->flux_1au * ratio * ratio / SRP_C_M_S;

    /* Away from the Sun: to_sun points at it, so the sign is negative. The
     * extra 1/d turns to_sun into a direction. */
    *a_out = vec3_scale(to_sun, -(pressure * p->coeff * shadow) / d);
}

void srp_gradient(const SrpParams *p, Vec3d to_sun, double shadow,
                  double g_out[9])
{
    for (int k = 0; k < 9; k++) {
        g_out[k] = 0.0;
    }

    double d2 = vec3_norm_sq(to_sun);
    if (!(d2 > 0.0) || !(shadow > 0.0) || !(p->coeff > 0.0)) {
        return;
    }

    double d = sqrt(d2);
    double ratio = SRP_AU_M / d;
    double pressure = p->flux_1au * ratio * ratio / SRP_C_M_S;

    /* With a = -g * s / d^3 and s = R_sun - r, so ds/dr = -I:
     *
     *     da_i/dr_j = g * ( delta_ij / d^3 - 3 s_i s_j / d^5 )
     *
     * The two sign flips - one from the direction being away from the Sun,
     * one from differentiating through s = R - r - cancel, which is exactly
     * the kind of thing core/field.c had to say out loud about harmonics.
     *
     * g is the whole constant flux_1au * AU^2 / c * coeff * shadow, so the
     * inverse square lives in the d^-3 below rather than being counted twice.
     * Recovered from `pressure` by multiplying its 1/d^2 back out, which
     * keeps the same well-scaled ratio srp_accel uses instead of forming
     * AU * AU directly. The first version of this line dropped the d^2 and
     * produced a gradient 22 orders too small; the finite-difference check
     * caught it immediately, which is the entire reason that check exists. */
    double g = pressure * d2 * p->coeff * shadow;
    double inv_d3 = 1.0 / (d2 * d);
    double three_over_d5 = 3.0 * inv_d3 / d2;

    /* Upper triangle then mirrored, as field_gradient does and for the same
     * reason: s.x*s.y and s.y*s.x are equal in exact arithmetic and can
     * differ in the last bit, and a caller entitled to a symmetric matrix
     * should get one. */
    g_out[0] = g * (inv_d3 - three_over_d5 * to_sun.x * to_sun.x);
    g_out[4] = g * (inv_d3 - three_over_d5 * to_sun.y * to_sun.y);
    g_out[8] = g * (inv_d3 - three_over_d5 * to_sun.z * to_sun.z);

    g_out[1] = g * (-three_over_d5 * to_sun.x * to_sun.y);
    g_out[2] = g * (-three_over_d5 * to_sun.x * to_sun.z);
    g_out[5] = g * (-three_over_d5 * to_sun.y * to_sun.z);

    g_out[3] = g_out[1];
    g_out[6] = g_out[2];
    g_out[7] = g_out[5];
}
