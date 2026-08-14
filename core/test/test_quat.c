/* core/quat.h (ROADMAP K3): vector rotation by unit quaternion.
 *
 * Every check here is pure algebra, independent of body_rotation.c's IAU
 * model - that module is checked separately (test_body_rotation.c) against
 * this one, not the other way around. */

#include "quat.h"
#include "test.h"

#include <math.h>

static int close(double a, double b, double tol)
{
    return fabs(a - b) < tol;
}

static int vec_close(Vec3d a, Vec3d b, double tol)
{
    return close(a.x, b.x, tol) && close(a.y, b.y, tol) && close(a.z, b.z, tol);
}

int main(void)
{
    /* Identity leaves every vector bit-exact: u=(0,0,0) makes both cross
     * products the zero vector with nothing to round. */
    {
        Vec3d v = vec3(3.7, -2.1, 9.4);
        Vec3d out = quat_rotate(quat_identity(), v);
        CHECK_BITS_EQ(out.x, v.x);
        CHECK_BITS_EQ(out.y, v.y);
        CHECK_BITS_EQ(out.z, v.z);
    }

    /* +90 degrees about z, built directly from the standard axis-angle
     * quaternion (cos(theta/2), axis*sin(theta/2)) rather than through
     * body_rotation.c: x -> y, y -> -x, z fixed. Hand-derived and checked
     * against this formula once (see ROADMAP K3) before trusting it here. */
    {
        double h = sqrt(2.0) / 2.0; /* cos(45deg) == sin(45deg) */
        Quat q = { h, 0.0, 0.0, h };

        Vec3d x_out = quat_rotate(q, vec3(1.0, 0.0, 0.0));
        Vec3d y_out = quat_rotate(q, vec3(0.0, 1.0, 0.0));
        Vec3d z_out = quat_rotate(q, vec3(0.0, 0.0, 1.0));

        CHECK(vec_close(x_out, vec3(0.0, 1.0, 0.0), 1e-12));
        CHECK(vec_close(y_out, vec3(-1.0, 0.0, 0.0), 1e-12));
        CHECK(vec_close(z_out, vec3(0.0, 0.0, 1.0), 1e-12));
    }

    /* Round trip through the conjugate, for a rotation with no special
     * symmetry and a vector with no zero components - the general case,
     * not one that happens to work because an axis lines up. */
    {
        Quat q = quat_normalize((Quat){ 0.4, -0.3, 0.7, 0.2 });
        Vec3d v = vec3(1.3, -4.2, 2.9);

        Vec3d forward = quat_rotate(q, v);
        Vec3d back = quat_rotate(quat_conjugate(q), forward);

        CHECK(vec_close(back, v, 1e-12));

        /* A unit quaternion's rotation preserves length - a check that does
         * not depend on trusting the formula's derivation, only on what a
         * rotation is. */
        CHECK(close(vec3_norm(forward), vec3_norm(v), 1e-12));
    }

    /* quat_rotate's closed form (see quat.h) is derived assuming |q| = 1;
     * fed a quaternion that is not unit length, it does not preserve
     * length. Not a bug to work around - the reason quat_normalize exists
     * for anything read back from a Chebyshev fit, which is only
     * approximately unit length between nodes. Loose rather than tied to a
     * specific formula: what matters is that skipping normalize is
     * detectably wrong, not what shape the error takes. */
    {
        Quat q = { 0.4, -0.3, 0.7, 0.2 };
        CHECK(fabs(quat_norm_sq(q) - 1.0) > 0.1); /* the test needs it off */
        Vec3d v = vec3(1.3, -4.2, 2.9);

        Vec3d not_normalized = quat_rotate(q, v);
        CHECK(!close(vec3_norm(not_normalized), vec3_norm(v), 1e-6));

        Vec3d normalized = quat_rotate(quat_normalize(q), v);
        CHECK(close(vec3_norm(normalized), vec3_norm(v), 1e-12));
    }

    return TEST_RESULT();
}
