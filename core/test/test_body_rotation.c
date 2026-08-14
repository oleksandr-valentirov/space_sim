/* core/offline/body_rotation.c (ROADMAP K3): the IAU mean pole/prime-
 * meridian model, checked against two things that do not depend on
 * trusting mat3_to_quat or the 3-1-3 matrix product: the textbook pole-
 * direction identity, computed independently here from the same cited
 * polynomial, and the sidereal rotation rate already committed in
 * data/horizons/obj_earth.txt and obj_moon.txt - an external oracle this
 * file did not have to invent. */

#include "body_rotation.h"
#include "test.h"

#include <math.h>

#define DAY 86400.0
#define CENTURY (36525.0 * DAY)
#define DEG (3.14159265358979323846 / 180.0)

static int close(double a, double b, double tol)
{
    return fabs(a - b) < tol;
}

/* Independent of body_rotation.c: the pole direction from (alpha0, delta0)
 * by the plain spherical-to-Cartesian formula, not through any rotation
 * matrix. */
static Vec3d pole_direction(double ra0, double ra1, double dec0, double dec1,
                            double t)
{
    double century = t / CENTURY;
    double alpha0 = (ra0 + ra1 * century) * DEG;
    double delta0 = (dec0 + dec1 * century) * DEG;
    return vec3(cos(delta0) * cos(alpha0), cos(delta0) * sin(alpha0),
               sin(delta0));
}

/* Angular rate implied by the quaternion at t, from a small central
 * difference applied to the body-fixed x axis. Dominated by the prime
 * meridian's own spin (deg/day) rather than the pole's centuries-scale
 * precession, so it isolates the part of the model the pole check above
 * does not touch. */
static double spin_rate(const char *name, double t)
{
    double dt = 1.0;
    Quat q0, q1;
    CHECK(body_rotation_of(name, t - dt, &q0) == CORE_OK);
    CHECK(body_rotation_of(name, t + dt, &q1) == CORE_OK);

    Vec3d v0 = quat_rotate(q0, vec3(1.0, 0.0, 0.0));
    Vec3d v1 = quat_rotate(q1, vec3(1.0, 0.0, 0.0));

    double cos_angle = vec3_dot(v0, v1) / (vec3_norm(v0) * vec3_norm(v1));
    if (cos_angle > 1.0) cos_angle = 1.0;
    if (cos_angle < -1.0) cos_angle = -1.0;
    return acos(cos_angle) / (2.0 * dt);
}

int main(void)
{
    /* Unmodelled body: identity, bit for bit - "not modelled" has to read
     * back unambiguously, not as a rotation that happens to be small. */
    {
        Quat q;
        CHECK(body_rotation_of("jupiter_bary", 12345.0, &q) == CORE_OK);
        CHECK_BITS_EQ(q.w, 1.0);
        CHECK_BITS_EQ(q.x, 0.0);
        CHECK_BITS_EQ(q.y, 0.0);
        CHECK_BITS_EQ(q.z, 0.0);
    }

    double epochs[3] = { 0.0, 365.25 * DAY, 10.0 * 365.25 * DAY };

    /* Earth: NAIF pck00011.tpc, BODY399_POLE_RA/POLE_DEC (body_rotation.c). */
    for (int i = 0; i < 3; i++) {
        double t = epochs[i];
        Quat q;
        CHECK(body_rotation_of("earth", t, &q) == CORE_OK);

        CHECK(close(quat_norm_sq(q), 1.0, 1e-9));

        Vec3d pole_from_quat = quat_rotate(q, vec3(0.0, 0.0, 1.0));
        Vec3d pole_expected = pole_direction(0.0, -0.641, 90.0, -0.557, t);
        CHECK(close(pole_from_quat.x, pole_expected.x, 1e-9));
        CHECK(close(pole_from_quat.y, pole_expected.y, 1e-9));
        CHECK(close(pole_from_quat.z, pole_expected.z, 1e-9));
    }

    /* Earth's sidereal rate, obj_earth.txt: "Rot. Rate (rad/s) =
     * 0.00007292115". 0.3% tolerance: the finite difference sees the pole's
     * own (tiny, centuries-scale) precession too, not only the spin. */
    CHECK(close(spin_rate("earth", 365.25 * DAY), 0.00007292115,
               0.003 * 0.00007292115));

    /* Moon: same two checks, obj_moon.txt: "Sid. rot. rate, rad/s =
     * 0.0000026617". */
    for (int i = 0; i < 3; i++) {
        double t = epochs[i];
        Quat q;
        CHECK(body_rotation_of("moon", t, &q) == CORE_OK);

        CHECK(close(quat_norm_sq(q), 1.0, 1e-9));

        Vec3d pole_from_quat = quat_rotate(q, vec3(0.0, 0.0, 1.0));
        Vec3d pole_expected =
            pole_direction(269.9949, 0.0031, 66.5392, 0.0130, t);
        CHECK(close(pole_from_quat.x, pole_expected.x, 1e-9));
        CHECK(close(pole_from_quat.y, pole_expected.y, 1e-9));
        CHECK(close(pole_from_quat.z, pole_expected.z, 1e-9));
    }

    CHECK(close(spin_rate("moon", 365.25 * DAY), 0.0000026617,
               0.003 * 0.0000026617));

    return TEST_RESULT();
}
