#include "nbody.h"

#include "body_rotation.h"
#include "quat.h"

#include "dop853_coeffs.h"

#include <math.h>

#define SAFETY     0.9
#define MIN_SCALE  0.2
#define MAX_SCALE  10.0
#define DEFAULT_MAX_STEPS 10000000L

typedef struct {
    Vec3d dr[NBODY_MAX];
    Vec3d dv[NBODY_MAX];
} SystemDeriv;

/* The body's orientation at t, or the identity when nothing says otherwise
 * (ROADMAP K5e).
 *
 * The runtime reads this from the asset (core/field.c); the cooker is what
 * WRITES the asset, so it goes to the model directly. Both end up applying
 * the same rotation, which is what keeps the field a vessel flies in and the
 * field the bodies moved under the same field.
 *
 * A body with no name, no model, or a failed lookup gets the identity: for a
 * zonal field on a body whose pole is the frame's z axis - Earth, here -
 * that is exactly what the assumption used to be, so nothing about the
 * Earth's contribution changes by more than the precession since J2000. */
static Quat body_frame_of(const NBodySystem *sys, size_t body, double t)
{
    if (sys->name[body] == NULL) {
        return quat_identity();
    }

    Quat q;
    if (body_rotation_of(sys->name[body], t, &q) != CORE_OK) {
        return quat_identity();
    }
    return q;
}

void nbody_accel(const NBodySystem *sys, const State *states, Vec3d *acc_out)
{
    for (size_t i = 0; i < sys->n; i++) {
        Vec3d a = vec3_zero();

        for (size_t j = 0; j < sys->n; j++) {
            if (j == i) {
                continue;
            }

            Vec3d d = vec3_sub(states[j].r, states[i].r);
            double d2 = vec3_norm_sq(d);
            double dn = sqrt(d2);

            a = vec3_add_scaled(a, d, sys->mu[j] / (d2 * dn));
        }

        acc_out[i] = a;
    }

    /* One body's oblateness (ROADMAP K2). d is Earth-relative position taken
     * straight from the inertial states, which is only correct because K2
     * assumes the oblate body's pole is fixed along the frame's own z axis.
     * That is exact for ICRF at J2000 by construction and decays at 0.557
     * degrees per century: 6.6 arcsec over the committed 120-day fixture,
     * 3.3 arcmin over the ten-year ex_ephspan diagnostic.
     *
     * What that costs is measured rather than argued (ROADMAP K3a): forcing
     * the whole ten-year cook through the worst-case 3.3 arcmin tilt moves
     * the Moon's geocentric error from 3.454e4 m to 3.485e4 m, under 1%.
     * So the z-axis assumption is not what limits this model, and giving
     * bodies a real orientation (K3b) is needed for tesseral terms and a
     * rotating atmosphere, not for the accuracy of the cook.
     *
     * Newton's third law, not assumed but carried through explicitly: the
     * force on j from the oblate body's field scales with mu_e (it is that
     * body's field), so the reaction on the oblate body scales the same
     * acceleration by -mu_j/mu_e to turn "j's acceleration" into "e's",
     * exactly as nbody_energy's matching potential term requires for the
     * two to stay consistent. */
    for (size_t e = 0; e < sys->n; e++) {
        if (sys->field[e] == NULL || sys->field[e]->degree < 2) {
            continue;
        }

        for (size_t j = 0; j < sys->n; j++) {
            if (j == e) {
                continue;
            }

            Vec3d d = vec3_sub(states[j].r, states[e].r);

            Quat q = body_frame_of(sys, e, states[e].t);
            Vec3d local = quat_rotate(quat_conjugate(q), d);

            Vec3d a_j2;
            harmonics_accel(sys->field[e], local, sys->mu[e], &a_j2);
            a_j2 = quat_rotate(q, a_j2);

            acc_out[j] = vec3_add(acc_out[j], a_j2);
            acc_out[e] = vec3_add_scaled(acc_out[e], a_j2,
                                         -sys->mu[j] / sys->mu[e]);
        }
    }
}

double nbody_energy(const NBodySystem *sys, const State *states)
{
    double kinetic = 0.0;
    for (size_t i = 0; i < sys->n; i++) {
        kinetic += 0.5 * sys->mu[i] * vec3_norm_sq(states[i].v);
    }

    double potential = 0.0;
    for (size_t i = 0; i < sys->n; i++) {
        for (size_t j = i + 1; j < sys->n; j++) {
            double d = vec3_distance(states[i].r, states[j].r);
            potential -= sys->mu[i] * sys->mu[j] / d;
        }
    }

    /* The one potential whose gradient is nbody_accel's J2 term - literally
     * the same derivation, not a separately-invented energy formula: a_j =
     * +grad_j U_pines(d) and a_e = -grad_e(mu_j * U_pines(d))/mu_e both fall
     * out of PE_j2 = -mu_j * U_pines(d), which is why -mu_j * u is what gets
     * subtracted here. Missing this term would not make the physics wrong,
     * only this diagnostic - a fixed 2-body energy check would see a drift
     * that is really just the J2 potential swinging with distance, not the
     * integrator failing. */
    for (size_t e = 0; e < sys->n; e++) {
        if (sys->field[e] == NULL || sys->field[e]->degree < 2) {
            continue;
        }
        for (size_t j = 0; j < sys->n; j++) {
            if (j == e) {
                continue;
            }
            Quat q = body_frame_of(sys, e, states[e].t);
            Vec3d local = quat_rotate(quat_conjugate(q),
                                      vec3_sub(states[j].r, states[e].r));
            double u;
            harmonics_potential(sys->field[e], local, sys->mu[e], &u);
            potential -= sys->mu[j] * u;
        }
    }

    return kinetic + potential;
}

Vec3d nbody_barycentre(const NBodySystem *sys, const State *states)
{
    Vec3d weighted = vec3_zero();
    double total = 0.0;

    for (size_t i = 0; i < sys->n; i++) {
        weighted = vec3_add_scaled(weighted, states[i].r, sys->mu[i]);
        total += sys->mu[i];
    }

    if (total == 0.0) {
        return vec3_zero();
    }
    return vec3_scale(weighted, 1.0 / total);
}

Vec3d nbody_momentum_velocity(const NBodySystem *sys, const State *states)
{
    Vec3d weighted = vec3_zero();
    double total = 0.0;

    for (size_t i = 0; i < sys->n; i++) {
        weighted = vec3_add_scaled(weighted, states[i].v, sys->mu[i]);
        total += sys->mu[i];
    }

    if (total == 0.0) {
        return vec3_zero();
    }
    return vec3_scale(weighted, 1.0 / total);
}

void nbody_anchor_barycentre(const NBodySystem *sys, State *states)
{
    Vec3d drift = nbody_momentum_velocity(sys, states);

    for (size_t i = 0; i < sys->n; i++) {
        states[i].v = vec3_sub(states[i].v, drift);
    }
}

static double dabs(double x)
{
    return x < 0.0 ? -x : x;
}

/* See dop853.c: pow() is unavailable in the deterministic zone and the
 * exponent is exactly 1/8. Kept identical here so both integrators make the
 * same step decisions from the same error. */
static double eighth_root(double x)
{
    return sqrt(sqrt(sqrt(x)));
}

static double system_err_norm_sq(const Vec3d *er, const Vec3d *ev, size_t n,
                                 double scale_r, double scale_v)
{
    double sum = 0.0;

    for (size_t i = 0; i < n; i++) {
        double xr = er[i].x / scale_r;
        double yr = er[i].y / scale_r;
        double zr = er[i].z / scale_r;
        double xv = ev[i].x / scale_v;
        double yv = ev[i].y / scale_v;
        double zv = ev[i].z / scale_v;

        sum += xr * xr + yr * yr + zr * zr + xv * xv + yv * yv + zv * zv;
    }

    return sum;
}

static void system_try_step(const NBodySystem *sys, const State *y, double h,
                            double tol_m, SystemDeriv *k,
                            State *out, double *error_norm, long *n_evals)
{
    size_t n = sys->n;
    State stage[NBODY_MAX];

    for (int i = 1; i < DOP853_STAGES; i++) {
        for (size_t b = 0; b < n; b++) {
            Vec3d dr = vec3_zero();
            Vec3d dv = vec3_zero();

            for (int j = 0; j < i; j++) {
                double a = DOP853_A[i][j];
                dr = vec3_add_scaled(dr, k[j].dr[b], a);
                dv = vec3_add_scaled(dv, k[j].dv[b], a);
            }

            stage[b].r = vec3_add_scaled(y[b].r, dr, h);
            stage[b].v = vec3_add_scaled(y[b].v, dv, h);
            stage[b].t = y[b].t + DOP853_C[i] * h;

            k[i].dr[b] = stage[b].v;
        }

        nbody_accel(sys, stage, k[i].dv);
        (*n_evals)++;
    }

    for (size_t b = 0; b < n; b++) {
        Vec3d sum_r = vec3_zero();
        Vec3d sum_v = vec3_zero();

        for (int j = 0; j < DOP853_STAGES; j++) {
            sum_r = vec3_add_scaled(sum_r, k[j].dr[b], DOP853_B[j]);
            sum_v = vec3_add_scaled(sum_v, k[j].dv[b], DOP853_B[j]);
        }

        out[b].r = vec3_add_scaled(y[b].r, sum_r, h);
        out[b].v = vec3_add_scaled(y[b].v, sum_v, h);
        out[b].t = y[b].t + h;

        k[DOP853_STAGES].dr[b] = out[b].v;
    }

    nbody_accel(sys, out, k[DOP853_STAGES].dv);
    (*n_evals)++;

    Vec3d e5r[NBODY_MAX], e5v[NBODY_MAX], e3r[NBODY_MAX], e3v[NBODY_MAX];
    for (size_t b = 0; b < n; b++) {
        e5r[b] = vec3_zero();
        e5v[b] = vec3_zero();
        e3r[b] = vec3_zero();
        e3v[b] = vec3_zero();

        for (int j = 0; j <= DOP853_STAGES; j++) {
            e5r[b] = vec3_add_scaled(e5r[b], k[j].dr[b], DOP853_E5[j]);
            e5v[b] = vec3_add_scaled(e5v[b], k[j].dv[b], DOP853_E5[j]);
            e3r[b] = vec3_add_scaled(e3r[b], k[j].dr[b], DOP853_E3[j]);
            e3v[b] = vec3_add_scaled(e3v[b], k[j].dv[b], DOP853_E3[j]);
        }
    }

    double scale_r = tol_m;
    double scale_v = tol_m / dabs(h);

    double n5 = system_err_norm_sq(e5r, e5v, n, scale_r, scale_v);
    double n3 = system_err_norm_sq(e3r, e3v, n, scale_r, scale_v);

    double denom = n5 + 0.01 * n3;
    if (denom <= 0.0) {
        *error_norm = 0.0;
    } else {
        *error_norm = dabs(h) * n5 / sqrt(denom * 6.0 * (double)n);
    }
}

CoreResult nbody_integrate(const NBodySystem *sys, const State *in,
                           double t_end, const Dop853Config *cfg,
                           Dop853State *io, State *out)
{
    if (sys == NULL || in == NULL || cfg == NULL || io == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (sys->n == 0 || sys->n > NBODY_MAX || !(cfg->tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    size_t n = sys->n;
    State current[NBODY_MAX];
    for (size_t b = 0; b < n; b++) {
        current[b] = in[b];
    }

    double t0 = current[0].t;
    if (t_end == t0) {
        for (size_t b = 0; b < n; b++) {
            out[b] = current[b];
        }
        return CORE_OK;
    }

    double direction = t_end > t0 ? 1.0 : -1.0;
    long max_steps = cfg->max_steps > 0 ? cfg->max_steps : DEFAULT_MAX_STEPS;

    static SystemDeriv k[DOP853_STAGES + 1];
    for (size_t b = 0; b < n; b++) {
        k[0].dr[b] = current[b].v;
    }
    nbody_accel(sys, current, k[0].dv);
    io->n_evals++;

    double h = io->h > 0.0 ? io->h
             : (cfg->h_init > 0.0 ? cfg->h_init : 60.0);

    /* See the same clamp in dop853.c: without a ceiling, a caller that
     * integrates in legs shorter than the natural step compounds h without
     * bound until it overflows, after which a rejected step can never shrink
     * again. The ephemeris cooker is exactly such a caller. */
    double h_ceiling = cfg->h_max > 0.0 ? cfg->h_max : dabs(t_end - t0);
    if (h > h_ceiling) {
        h = h_ceiling;
    }

    long steps = 0;

    while ((t_end - current[0].t) * direction > 0.0) {
        if (++steps > max_steps) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }
        if (cfg->h_min > 0.0 && h < cfg->h_min) {
            return CORE_ERR_TOLERANCE_NOT_MET;
        }

        double remaining = (t_end - current[0].t) * direction;
        double h_used = h < remaining ? h : remaining;

        State candidate[NBODY_MAX];
        double error_norm;
        system_try_step(sys, current, h_used * direction, cfg->tol_m, k,
                        candidate, &error_norm, &io->n_evals);

        double factor = error_norm == 0.0
                      ? MAX_SCALE
                      : SAFETY / eighth_root(error_norm);

        if (error_norm < 1.0) {
            for (size_t b = 0; b < n; b++) {
                current[b] = candidate[b];
            }
            io->n_accepted++;
            k[0] = k[DOP853_STAGES];

            if (factor > MAX_SCALE) {
                factor = MAX_SCALE;
            }
            h *= factor;
        } else {
            io->n_rejected++;

            if (factor > 1.0) {
                factor = 1.0;
            }
            if (factor < MIN_SCALE) {
                factor = MIN_SCALE;
            }
            h *= factor;
        }

        if (h > h_ceiling) {
            h = h_ceiling;
        }
    }

    for (size_t b = 0; b < n; b++) {
        current[b].t = t_end;
        out[b] = current[b];
    }
    io->h = h;
    return CORE_OK;
}
