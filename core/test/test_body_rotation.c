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

static int approx(double a, double b, double tol)
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

/* Which way the body turns about its own pole: +1 prograde, -1 retrograde.
 *
 * A separate check from spin_rate because that one cannot see it - it
 * measures an unsigned angle between two directions, so flipping the sign
 * of the prime-meridian rate would leave it passing. Both bodies modelled
 * here rotate prograde, and getting that backwards is exactly the kind of
 * convention error a passive-versus-active rotation matrix invites. */
static double spin_sense(const char *name, double t)
{
    Quat q0, q1;
    CHECK(body_rotation_of(name, t, &q0) == CORE_OK);
    CHECK(body_rotation_of(name, t + 60.0, &q1) == CORE_OK);

    Vec3d pole = quat_rotate(q0, vec3(0.0, 0.0, 1.0));
    Vec3d m0 = quat_rotate(q0, vec3(1.0, 0.0, 0.0));
    Vec3d m1 = quat_rotate(q1, vec3(1.0, 0.0, 0.0));

    return vec3_dot(vec3_cross(m0, m1), pole);
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

        CHECK(approx(quat_norm_sq(q), 1.0, 1e-9));

        Vec3d pole_from_quat = quat_rotate(q, vec3(0.0, 0.0, 1.0));
        Vec3d pole_expected = pole_direction(0.0, -0.641, 90.0, -0.557, t);
        CHECK(approx(pole_from_quat.x, pole_expected.x, 1e-9));
        CHECK(approx(pole_from_quat.y, pole_expected.y, 1e-9));
        CHECK(approx(pole_from_quat.z, pole_expected.z, 1e-9));
    }

    /* Earth's sidereal rate, obj_earth.txt: "Rot. Rate (rad/s) =
     * 0.00007292115". 0.3% tolerance: the finite difference sees the pole's
     * own (tiny, centuries-scale) precession too, not only the spin. */
    CHECK(approx(spin_rate("earth", 365.25 * DAY), 0.00007292115,
               0.003 * 0.00007292115));

    /* Moon: same two checks, obj_moon.txt: "Sid. rot. rate, rad/s =
     * 0.0000026617". */
    for (int i = 0; i < 3; i++) {
        double t = epochs[i];
        Quat q;
        CHECK(body_rotation_of("moon", t, &q) == CORE_OK);

        CHECK(approx(quat_norm_sq(q), 1.0, 1e-9));

        Vec3d pole_from_quat = quat_rotate(q, vec3(0.0, 0.0, 1.0));
        Vec3d pole_expected =
            pole_direction(269.9949, 0.0031, 66.5392, 0.0130, t);
        CHECK(approx(pole_from_quat.x, pole_expected.x, 1e-9));
        CHECK(approx(pole_from_quat.y, pole_expected.y, 1e-9));
        CHECK(approx(pole_from_quat.z, pole_expected.z, 1e-9));
    }

    CHECK(approx(spin_rate("moon", 365.25 * DAY), 0.0000026617,
               0.003 * 0.0000026617));

    /* Both prograde. Measured, not assumed: see spin_sense. */
    CHECK(spin_sense("earth", 365.25 * DAY) > 0.0);
    CHECK(spin_sense("moon", 365.25 * DAY) > 0.0);

    /* Earth's prime meridian against the Earth Rotation Angle (ROADMAP K3b).
     *
     * Two epochs a century apart, and the second is the one with teeth: the
     * phase is easy to match and the rate is where the model this replaced
     * was actually wrong (833 arcsec per century), and where the node-drift
     * term of body_rotation.c's derivation could still have the wrong sign -
     * which would show up here as 0.641 degrees, or 2300 arcsec, per
     * century.
     *
     * ERA is restated here from its own definition rather than shared with
     * the module under test, and the clock conversion with it. Measured: 0.0
     * arcsec at J2000, 1.7 at J2100, 7.0 at J2200 - a residual that grows
     * quadratically because it is the pole's own tilt away from z entering
     * this longitude measurement, not the meridian model drifting. */
    {
        double delta_t = 32.184 + 32.0 - 0.3554;   /* TT - UT1 at J2000, s */

        double checks[2] = { 0.0, 36525.0 * DAY };
        double tolerance[2] = { 2.0, 5.0 };        /* arcseconds */

        for (int i = 0; i < 2; i++) {
            double t = checks[i];
            double ut1_days = (t - delta_t) / DAY;
            double era = 360.0 * (0.7790572732640
                                  + 1.00273781191135448 * ut1_days);

            Quat q;
            CHECK(body_rotation_of("earth", t, &q) == CORE_OK);
            Vec3d m = quat_rotate(q, vec3(1.0, 0.0, 0.0));
            double model = atan2(m.y, m.x) / DEG;

            double difference = fmod(model - era, 360.0);
            if (difference > 180.0) difference -= 360.0;
            if (difference < -180.0) difference += 360.0;

            CHECK(approx(difference * 3600.0, 0.0, tolerance[i]));
        }
    }

    return TEST_RESULT();
}
