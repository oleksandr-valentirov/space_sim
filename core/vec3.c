#include "vec3.h"

Vec3d vec3_sum_ordered(const Vec3d *v, size_t n)
{
    Vec3d sum = vec3_zero();

    for (size_t i = 0; i < n; i++) {
        sum = vec3_add(sum, v[i]);
    }

    return sum;
}
