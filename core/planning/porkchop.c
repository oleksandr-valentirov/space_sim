#include "porkchop.h"

#include "lambert.h"

CoreResult porkchop_compute(BodyStateFunc depart, void *depart_ctx,
                            BodyStateFunc arrive, void *arrive_ctx,
                            double mu, int prograde,
                            const double *t1_grid, size_t n_t1,
                            const double *tof_grid, size_t n_tof,
                            PorkchopPoint *out, size_t out_cap,
                            size_t *out_count)
{
    if (depart == NULL || arrive == NULL || t1_grid == NULL
        || tof_grid == NULL || out == NULL || out_count == NULL
        || n_t1 == 0 || n_tof == 0 || !(mu > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    size_t n = 0;
    int buffer_full = 0;

    for (size_t i = 0; i < n_t1 && !buffer_full; i++) {
        double t1 = t1_grid[i];
        Vec3d r1, v1_body;
        if (depart(t1, depart_ctx, &r1, &v1_body) != CORE_OK) {
            continue;
        }

        for (size_t j = 0; j < n_tof; j++) {
            double tof = tof_grid[j];
            if (!(tof > 0.0)) {
                continue;
            }

            Vec3d r2, v2_body;
            if (arrive(t1 + tof, arrive_ctx, &r2, &v2_body) != CORE_OK) {
                continue;
            }

            Vec3d v1_transfer, v2_transfer;
            if (lambert_solve(r1, r2, tof, mu, prograde, 0,
                              &v1_transfer, &v2_transfer) != CORE_OK) {
                continue;
            }

            if (n >= out_cap) {
                buffer_full = 1;
                break;
            }

            out[n].t1 = t1;
            out[n].tof = tof;
            out[n].v_inf_depart = vec3_distance(v1_transfer, v1_body);
            out[n].v_inf_arrive = vec3_distance(v2_transfer, v2_body);
            n++;
        }
    }

    *out_count = n;
    return buffer_full ? CORE_ERR_BUFFER_TOO_SMALL : CORE_OK;
}
