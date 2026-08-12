#include "frame.h"

#include <string.h>

CoreResult frame_synodic(const EphemerisCtx *eph, int primary, int secondary,
                         double t, SynodicFrame *out)
{
    if (eph == NULL || out == NULL || primary == secondary) {
        return CORE_ERR_INVALID_ARG;
    }

    State p, s;
    if (eph_body_state(eph, primary, t, &p) != CORE_OK ||
        eph_body_state(eph, secondary, t, &s) != CORE_OK) {
        return CORE_ERR_INVALID_ARG;
    }

    double mu_p = eph_body_mu(eph, primary);
    double mu_s = eph_body_mu(eph, secondary);
    double total = mu_p + mu_s;
    if (!(total > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    memset(out, 0, sizeof *out);

    Vec3d d = vec3_sub(s.r, p.r);
    Vec3d d_rate = vec3_sub(s.v, p.v);

    double length = vec3_norm(d);
    if (!(length > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    Vec3d h = vec3_cross(d, d_rate);
    double h_norm = vec3_norm(h);
    if (!(h_norm > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    out->length = length;
    out->length_rate = vec3_dot(d, d_rate) / length;

    out->x = vec3_scale(d, 1.0 / length);
    out->z = vec3_scale(h, 1.0 / h_norm);
    out->y = vec3_cross(out->z, out->x);

    /* omega = h / L^2 is the rotation that carries x with the separation
     * vector exactly: (d x d_dot) x d / L^3 is d_dot/L - x (dL/dt)/L, which is
     * dx/dt. It leaves z alone, which is the approximation named in the
     * header. */
    out->omega = vec3_scale(h, 1.0 / (length * length));
    out->rate = h_norm / (length * length);

    out->mu = mu_s / total;

    /* The CR3BP origin is the barycentre of the pair, so P lands at -mu and S
     * at 1 - mu. */
    double w_p = mu_p / total;
    double w_s = mu_s / total;
    out->origin = vec3_add(vec3_scale(p.r, w_p), vec3_scale(s.r, w_s));
    out->origin_rate = vec3_add(vec3_scale(p.v, w_p), vec3_scale(s.v, w_s));

    out->t = t;
    return CORE_OK;
}

/* Components in the frame's basis, assembled into an inertial vector. */
static Vec3d compose(const SynodicFrame *f, Vec3d q)
{
    Vec3d r = vec3_scale(f->x, q.x);
    r = vec3_add_scaled(r, f->y, q.y);
    r = vec3_add_scaled(r, f->z, q.z);
    return r;
}

static Vec3d decompose(const SynodicFrame *f, Vec3d v)
{
    return vec3(vec3_dot(v, f->x), vec3_dot(v, f->y), vec3_dot(v, f->z));
}

void frame_to_inertial(const SynodicFrame *f, const State *in, State *out)
{
    Vec3d offset = vec3_scale(compose(f, in->r), f->length);

    /* Three contributions to the velocity, and each one matters:
     *   - the frame turning under a fixed point,          omega x offset
     *   - the frame stretching,                           (dL/dt / L) offset
     *   - the motion in the frame itself, whose time unit is 1/rate.
     * The middle one has no counterpart in the CR3BP, where L is constant. */
    Vec3d rotation = vec3_cross(f->omega, offset);
    Vec3d stretch = vec3_scale(offset, f->length_rate / f->length);
    Vec3d relative = vec3_scale(compose(f, in->v), f->length * f->rate);

    out->r = vec3_add(f->origin, offset);
    out->v = vec3_add(f->origin_rate,
                      vec3_add(rotation, vec3_add(stretch, relative)));
    out->t = f->t;
}

void frame_from_inertial(const SynodicFrame *f, const State *in, State *out)
{
    Vec3d offset = vec3_sub(in->r, f->origin);
    Vec3d velocity = vec3_sub(in->v, f->origin_rate);

    Vec3d rotation = vec3_cross(f->omega, offset);
    Vec3d stretch = vec3_scale(offset, f->length_rate / f->length);
    Vec3d relative = vec3_sub(velocity, vec3_add(rotation, stretch));

    out->r = vec3_scale(decompose(f, offset), 1.0 / f->length);
    out->v = vec3_scale(decompose(f, relative),
                        1.0 / (f->length * f->rate));
    out->t = 0.0;
}
