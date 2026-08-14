/* Unit quaternions for body orientation (ROADMAP K3).
 *
 * Exists so a body's rotation can be fit to Chebyshev coefficients the same
 * way its position already is (PROJECT.md section 4: "матриці обертання тіл
 * ... теж чебишевськими поліномами"), and so applying that rotation at
 * runtime never needs trigonometry - vector rotation by a quaternion is
 * pure + - * /, and even the renormalisation a fitted-then-evaluated
 * quaternion needs (the fit is only approximately unit length between
 * nodes) is +, -, *, / and sqrt. Building the quaternion from IAU pole and
 * prime-meridian angles is a different problem with a different answer:
 * that needs sin/cos, so it lives in core/offline/body_rotation.c, on the
 * libm side of the boundary, and hands this module only the result.
 *
 * Convention: q rotates a vector's components from the body-fixed frame to
 * the inertial frame the ephemeris uses - "where does this body-fixed
 * direction point in the world". The reverse direction is quat_conjugate. */

#ifndef CORE_QUAT_H
#define CORE_QUAT_H

#include "vec3.h"

typedef struct {
    double w, x, y, z;
} Quat;

static inline Quat quat_identity(void)
{
    Quat q = { 1.0, 0.0, 0.0, 0.0 };
    return q;
}

static inline double quat_norm_sq(Quat q)
{
    return q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
}

/* Inverse of a unit quaternion. Only correct for unit length - the general
 * inverse needs a division by quat_norm_sq that nothing here needs, since
 * every quaternion this module produces is normalised first. */
static inline Quat quat_conjugate(Quat q)
{
    Quat r = { q.w, -q.x, -q.y, -q.z };
    return r;
}

/* Rescales to unit length. The one place division by (near) zero could
 * happen is a quaternion that was never a rotation to begin with - not a
 * case this module tries to survive gracefully, the same stance vec3.h
 * takes on a zero-length normalize. */
static inline Quat quat_normalize(Quat q)
{
    double n = sqrt(quat_norm_sq(q));
    double inv = 1.0 / n;
    Quat r = { q.w * inv, q.x * inv, q.y * inv, q.z * inv };
    return r;
}

/* v rotated by q, body-fixed to inertial per the file comment.
 *
 * v' = v + 2w(u x v) + 2 u x (u x v), u = (x, y, z) - the standard expansion
 * of q v q^-1 that never forms the intermediate quaternion product, so it
 * costs two cross products instead of two quaternion multiplications. No
 * trigonometry and no division, so it holds regardless of what put q there:
 * a freshly-built rotation or one read back through cheb_eval. */
static inline Vec3d quat_rotate(Quat q, Vec3d v)
{
    Vec3d u = vec3(q.x, q.y, q.z);
    Vec3d uv = vec3_cross(u, v);
    Vec3d uuv = vec3_cross(u, uv);
    Vec3d t = vec3_add_scaled(vec3_scale(uuv, 2.0), uv, 2.0 * q.w);
    return vec3_add(v, t);
}

#endif /* CORE_QUAT_H */
