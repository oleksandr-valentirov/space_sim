#include "station.h"

#include "stm.h"
#include "target.h"

#include <string.h>

#define DEFAULT_MAX_ITERATIONS 10
#define SECONDS_PER_YEAR 31557600.0   /* 365.25 days */

typedef struct {
    BlockAccelFunc f;
    void *ctx;
    Dop853Config cfg;
} Flight;

static CoreResult fly(Flight *flight, const State *in, double t_end,
                      State *out, double phi[STM_SIZE])
{
    Dop853State st;
    memset(&st, 0, sizeof st);

    return stm_integrate(flight->f, flight->ctx, in, t_end, &flight->cfg, &st,
                         out, phi);
}

CoreResult station_keep(BlockAccelFunc f, void *ctx,
                        const State *reference, const double *times, size_t n,
                        const State *initial,
                        const StationConfig *cfg, StationReport *out)
{
    if (f == NULL || reference == NULL || times == NULL || initial == NULL ||
        cfg == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (n < 2) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->tol_m > 0.0) || !(cfg->target_tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    memset(out, 0, sizeof *out);

    int interval = cfg->control_interval > 0 ? cfg->control_interval : 1;
    int horizon = cfg->horizon > 0 ? cfg->horizon : 1;
    int max_iterations = cfg->max_iterations > 0 ? cfg->max_iterations
                                                 : DEFAULT_MAX_ITERATIONS;

    Flight flight;
    flight.f = f;
    flight.ctx = ctx;
    memset(&flight.cfg, 0, sizeof flight.cfg);
    flight.cfg.tol_m = cfg->tol_m;
    flight.cfg.max_steps = 20000000;

    State vessel = *initial;
    vessel.t = times[0];

    for (size_t i = 0; i + 1 < n; i++) {
        /* A manoeuvre is allowed only at a control point, and only if there
         * is somewhere left to aim at. */
        if ((i % (size_t)interval) == 0) {
            size_t aim = i + (size_t)horizon;
            if (aim > n - 1) {
                aim = n - 1;
            }

            if (aim > i) {
                Vec3d before = vessel.v;

                State departure = vessel;
                if (target_hit(flight.f, flight.ctx, &flight.cfg, &departure,
                               times[aim], reference[aim].r, cfg->target_tol_m,
                               max_iterations, NULL) != CORE_OK) {
                    break;
                }

                double dv = vec3_norm(vec3_sub(departure.v, before));
                out->total_dv += dv;
                if (dv > out->largest_dv) {
                    out->largest_dv = dv;
                }
                out->manoeuvres++;

                vessel = departure;
            }
        }

        State next;
        if (fly(&flight, &vessel, times[i + 1], &next, NULL) != CORE_OK) {
            break;
        }
        vessel = next;

        double offset = vec3_distance(vessel.r, reference[i + 1].r);
        if (offset > out->worst_offset_m) {
            out->worst_offset_m = offset;
        }

        out->flown = times[i + 1] - times[0];

        if (i + 2 == n) {
            out->completed = 1;
        }
    }

    if (out->flown > 0.0) {
        out->per_year = out->total_dv * SECONDS_PER_YEAR / out->flown;
    }

    return CORE_OK;
}
