/* Three-dimensional vectors in double precision.
 *
 * Every operation here evaluates its arithmetic in the order written, and
 * nothing may reassociate it. That is not stylistic: floating point addition
 * is not associative, so a reordered sum is a different number, and
 * determinism is a hard requirement (PROJECT.md section 4). The compiler is
 * not allowed to reorder without -ffast-math, which the core never gets; the
 * remaining risk is a human deciding to "just parallelise the force loop".
 *
 * Units are metres and metres per second throughout. There is no separate
 * kilometre path: mixing the two silently is a classic source of wrong
 * trajectories, so the conversion happens once, when data is imported. */

#ifndef CORE_VEC3_H
#define CORE_VEC3_H

#include <math.h>
#include <stddef.h>

typedef struct {
    double x, y, z;
} Vec3d;

static inline Vec3d vec3(double x, double y, double z)
{
    Vec3d r = { x, y, z };
    return r;
}

static inline Vec3d vec3_zero(void)
{
    return vec3(0.0, 0.0, 0.0);
}

static inline Vec3d vec3_add(Vec3d a, Vec3d b)
{
    return vec3(a.x + b.x, a.y + b.y, a.z + b.z);
}

static inline Vec3d vec3_sub(Vec3d a, Vec3d b)
{
    return vec3(a.x - b.x, a.y - b.y, a.z - b.z);
}

static inline Vec3d vec3_neg(Vec3d a)
{
    return vec3(-a.x, -a.y, -a.z);
}

static inline Vec3d vec3_scale(Vec3d a, double s)
{
    return vec3(a.x * s, a.y * s, a.z * s);
}

/* a + b*s. The workhorse of every integrator stage: the product is formed
 * first, then added. Written as one function so the order is fixed in one
 * place instead of being retyped at each call site. */
static inline Vec3d vec3_add_scaled(Vec3d a, Vec3d b, double s)
{
    return vec3(a.x + b.x * s,
                a.y + b.y * s,
                a.z + b.z * s);
}

/* Component order x, y, z is part of the contract: changing it changes the
 * last bits of the result. */
static inline double vec3_dot(Vec3d a, Vec3d b)
{
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

static inline Vec3d vec3_cross(Vec3d a, Vec3d b)
{
    return vec3(a.y * b.z - a.z * b.y,
                a.z * b.x - a.x * b.z,
                a.x * b.y - a.y * b.x);
}

static inline double vec3_norm_sq(Vec3d a)
{
    return vec3_dot(a, a);
}

/* sqrt is correctly rounded per IEEE-754, which is why it is the one libm
 * name the deterministic zone may use. */
static inline double vec3_norm(Vec3d a)
{
    return sqrt(vec3_norm_sq(a));
}

static inline double vec3_distance(Vec3d a, Vec3d b)
{
    return vec3_norm(vec3_sub(a, b));
}

static inline int vec3_equal_bits(Vec3d a, Vec3d b)
{
    return a.x == b.x && a.y == b.y && a.z == b.z;
}

/* Sums in index order, 0 to n-1, and that order is part of the result.
 *
 * Use this for accumulating gravitational contributions rather than an ad hoc
 * loop, so there is a single place to look at when a sum has to be audited.
 * It must never be parallelised: a parallel reduction picks its own
 * association and produces different bits from run to run. */
Vec3d vec3_sum_ordered(const Vec3d *v, size_t n);

#endif /* CORE_VEC3_H */
