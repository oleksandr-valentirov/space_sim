#include "core.h"
#include "test.h"
#include "vec3.h"

int main(void)
{
    Vec3d a = vec3(1.0, 2.0, 3.0);
    Vec3d b = vec3(0.5, -1.0, 4.0);

    /* Exact values throughout: every operand here is representable, so the
     * results are exact and an epsilon comparison would only hide mistakes. */
    {
        Vec3d s = vec3_add(a, b);
        CHECK_BITS_EQ(s.x, 1.5);
        CHECK_BITS_EQ(s.y, 1.0);
        CHECK_BITS_EQ(s.z, 7.0);
    }
    {
        Vec3d d = vec3_sub(a, b);
        CHECK_BITS_EQ(d.x, 0.5);
        CHECK_BITS_EQ(d.y, 3.0);
        CHECK_BITS_EQ(d.z, -1.0);
    }
    {
        Vec3d s = vec3_scale(a, -2.0);
        CHECK_BITS_EQ(s.x, -2.0);
        CHECK_BITS_EQ(s.y, -4.0);
        CHECK_BITS_EQ(s.z, -6.0);
    }
    {
        /* a + b*s, the integrator workhorse. */
        Vec3d s = vec3_add_scaled(a, b, 2.0);
        CHECK_BITS_EQ(s.x, 2.0);
        CHECK_BITS_EQ(s.y, 0.0);
        CHECK_BITS_EQ(s.z, 11.0);
    }

    /* 1*0.5 + 2*(-1) + 3*4 = 10.5 */
    CHECK_BITS_EQ(vec3_dot(a, b), 10.5);

    {
        Vec3d c = vec3_cross(a, b);
        CHECK_BITS_EQ(c.x, 11.0);  /*  2*4  - 3*(-1) */
        CHECK_BITS_EQ(c.y, -2.5);  /*  3*0.5 - 1*4   */
        CHECK_BITS_EQ(c.z, -2.0);  /*  1*(-1) - 2*0.5 */

        /* Properties that catch a swapped term, which exact values alone
         * might not. */
        Vec3d back = vec3_cross(b, a);
        CHECK(vec3_equal_bits(back, vec3_neg(c)));
        CHECK_BITS_EQ(vec3_dot(c, a), 0.0);
        CHECK_BITS_EQ(vec3_dot(c, b), 0.0);
        CHECK(vec3_equal_bits(vec3_cross(a, a), vec3_zero()));
    }

    /* 3-4-5, so the norm is exact and sqrt has nothing to round. */
    {
        Vec3d p = vec3(3.0, 4.0, 0.0);
        CHECK_BITS_EQ(vec3_norm_sq(p), 25.0);
        CHECK_BITS_EQ(vec3_norm(p), 5.0);
        CHECK_BITS_EQ(vec3_distance(vec3(1.0, 1.0, 1.0), vec3(4.0, 5.0, 1.0)), 5.0);
    }

    /* The point of vec3_sum_ordered: index order is part of the answer.
     *
     * Forward, 1.0 is added to 1e16 first and vanishes (it sits below half an
     * ULP there), then 1e16 cancels and the sum is 0. Reversed, the two large
     * terms cancel first and the 1.0 survives. Same three numbers, same
     * operation, different results — which is why a parallel reduction over
     * this loop would break determinism. */
    {
        Vec3d fwd[3] = {
            vec3(1.0, 0.0, 0.0),
            vec3(1e16, 0.0, 0.0),
            vec3(-1e16, 0.0, 0.0),
        };
        Vec3d rev[3] = { fwd[2], fwd[1], fwd[0] };

        CHECK_BITS_EQ(vec3_sum_ordered(fwd, 3).x, 0.0);
        CHECK_BITS_EQ(vec3_sum_ordered(rev, 3).x, 1.0);

        CHECK(vec3_equal_bits(vec3_sum_ordered(fwd, 0), vec3_zero()));
    }

    /* State is a plain struct of doubles: it crosses the FFI boundary, so no
     * padding may creep in between the fields. */
    {
        CHECK(sizeof(State) == 7 * sizeof(double));
        CHECK(sizeof(Vec3d) == 3 * sizeof(double));

        State s = { vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0), 7.0 };
        CHECK_BITS_EQ(s.r.x, 1.0);
        CHECK_BITS_EQ(s.v.z, 6.0);
        CHECK_BITS_EQ(s.t, 7.0);
    }

    return TEST_RESULT();
}
