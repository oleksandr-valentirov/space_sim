#include "station.h"

#include "stm.h"

#include <string.h>

#define DEFAULT_MAX_ITERATIONS 10
#define SECONDS_PER_YEAR 31557600.0   /* 365.25 days */

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* Solve a x = b for a 3x3 a, Gaussian elimination with partial pivoting.
 * a and b are destroyed. Returns 0 if a is singular.
 *
 * Pivoting matters here rather than being routine: the matrix is
 * d(arrival position) / d(departure velocity), and over a horizon of one or
 * two revolutions its three directions differ in sensitivity by the
 * eigenvalue of the orbit - a factor of hundreds. */
static int solve3(double a[9], double b[3], double out[3])
{
    int row[3] = { 0, 1, 2 };

    for (int col = 0; col < 3; col++) {
        int best = col;
        double best_size = dabs(a[row[col] * 3 + col]);
        for (int k = col + 1; k < 3; k++) {
            double size = dabs(a[row[k] * 3 + col]);
            if (size > best_size) {
                best_size = size;
                best = k;
            }
        }

        if (best_size == 0.0) {
            return 0;
        }

        int swap = row[col];
        row[col] = row[best];
        row[best] = swap;

        double pivot = a[row[col] * 3 + col];

        for (int k = col + 1; k < 3; k++) {
            double factor = a[row[k] * 3 + col] / pivot;
            for (int j = col; j < 3; j++) {
                a[row[k] * 3 + j] -= factor * a[row[col] * 3 + j];
            }
            b[row[k]] -= factor * b[row[col]];
        }
    }

    for (int col = 2; col >= 0; col--) {
        double sum = b[row[col]];
        for (int j = col + 1; j < 3; j++) {
            sum -= a[row[col] * 3 + j] * out[j];
        }
        out[col] = sum / a[row[col] * 3 + col];
    }

    return 1;
}

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

/* Adjust the departure velocity so the position at t_aim is `aim`. */
static CoreResult target(Flight *flight, State *state, double t_aim, Vec3d aim,
                         double tol, int max_iterations)
{
    for (int i = 0; i < max_iterations; i++) {
        double phi[STM_SIZE];
        State arrival;

        CoreResult r = fly(flight, state, t_aim, &arrival, phi);
        if (r != CORE_OK) {
            return r;
        }

        Vec3d miss = vec3_sub(aim, arrival.r);
        if (vec3_norm(miss) < tol) {
            return CORE_OK;
        }

        double a[9];
        for (int row = 0; row < 3; row++) {
            for (int col = 0; col < 3; col++) {
                a[row * 3 + col] = phi[row * 6 + (col + 3)];
            }
        }

        double b[3] = { miss.x, miss.y, miss.z };
        double delta[3];
        if (!solve3(a, b, delta)) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        state->v.x += delta[0];
        state->v.y += delta[1];
        state->v.z += delta[2];
    }

    return CORE_ERR_TOLERANCE_NOT_MET;
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
                if (target(&flight, &departure, times[aim], reference[aim].r,
                           cfg->target_tol_m, max_iterations) != CORE_OK) {
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
