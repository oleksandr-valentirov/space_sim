#include "ephemeris.h"

#include "cheb.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct EphemerisCtx {
    unsigned n_bodies;
    unsigned n_intervals;
    unsigned degree;
    unsigned orient_degree;   /* 0 when no body carries orientation */
    unsigned n_orient;        /* how many bodies do */

    double t_begin;
    double interval;

    char   (*names)[EPH_NAME_SIZE];
    double  *mu;
    double  *radius;   /* metres, 0 where the asset does not say */
    double  *flux;     /* W/m^2 at 1 AU, 0 for a body that does not shine */
    double  *coeffs;   /* [interval][body][component][degree] */

    /* [interval][slot][w,x,y,z][orient_degree], where slot is the body's
     * position among the bodies that carry orientation, in body order.
     * orient_slot is -1 for a body that carries none. */
    int     *orient_slot;
    double  *orient;

    /* One per body, degree 0 where the asset says point mass. Stored
     * expanded rather than as the file's variable-length blocks: the file
     * is read once, and a reader that had to walk a triangular array to
     * answer eph_body_harmonics would be trading a few kilobytes for a
     * bug. */
    HarmonicsField *harmonics;

    /* One per body, no layers where the asset says airless (ROADMAP K7b).
     * Expanded for the same reason the harmonics are. */
    AtmosphereModel *atmosphere;
};

static int read_exact(FILE *f, void *dst, size_t n)
{
    return fread(dst, 1, n, f) == n;
}

CoreResult eph_load(const char *path, EphemerisCtx **out)
{
    if (path == NULL || out == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    *out = NULL;

    FILE *f = fopen(path, "rb");
    if (f == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    char magic[EPH_MAGIC_SIZE];
    unsigned version, n_bodies, n_intervals, degree, orient_degree;
    double t_begin, interval, sentinel;

    int header_ok =
        read_exact(f, magic, sizeof magic) &&
        memcmp(magic, EPH_MAGIC, sizeof magic) == 0 &&
        read_exact(f, &version, sizeof version) &&
        version == EPH_VERSION &&
        read_exact(f, &n_bodies, sizeof n_bodies) &&
        read_exact(f, &n_intervals, sizeof n_intervals) &&
        read_exact(f, &degree, sizeof degree) &&
        read_exact(f, &orient_degree, sizeof orient_degree) &&
        read_exact(f, &t_begin, sizeof t_begin) &&
        read_exact(f, &interval, sizeof interval) &&
        read_exact(f, &sentinel, sizeof sentinel);

    /* The sentinel is exactly 1.0 written as a double. If it does not read
     * back as 1.0, the file came from a machine that disagrees about byte
     * order or floating point layout, and every number after it is noise. */
    if (!header_ok || sentinel != 1.0 ||
        n_bodies == 0 || n_intervals == 0 || degree < 2 ||
        orient_degree == 1u || !(interval > 0.0)) {
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    EphemerisCtx *ctx = calloc(1, sizeof *ctx);
    if (ctx == NULL) {
        fclose(f);
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    ctx->n_bodies = n_bodies;
    ctx->n_intervals = n_intervals;
    ctx->degree = degree;
    ctx->orient_degree = orient_degree;
    ctx->t_begin = t_begin;
    ctx->interval = interval;

    size_t n_coeffs = (size_t)n_intervals * n_bodies * 3u * degree;

    ctx->names = calloc(n_bodies, sizeof *ctx->names);
    ctx->mu = calloc(n_bodies, sizeof *ctx->mu);
    ctx->radius = calloc(n_bodies, sizeof *ctx->radius);
    ctx->flux = calloc(n_bodies, sizeof *ctx->flux);
    ctx->coeffs = calloc(n_coeffs, sizeof *ctx->coeffs);
    ctx->harmonics = calloc(n_bodies, sizeof *ctx->harmonics);
    ctx->atmosphere = calloc(n_bodies, sizeof *ctx->atmosphere);
    ctx->orient_slot = calloc(n_bodies, sizeof *ctx->orient_slot);

    if (ctx->names == NULL || ctx->mu == NULL || ctx->radius == NULL ||
        ctx->flux == NULL || ctx->coeffs == NULL ||
        ctx->harmonics == NULL || ctx->atmosphere == NULL ||
        ctx->orient_slot == NULL) {
        eph_free(ctx);
        fclose(f);
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    for (unsigned b = 0; b < n_bodies; b++) {
        unsigned degree, has_orientation;

        if (!read_exact(f, ctx->names[b], EPH_NAME_SIZE) ||
            !read_exact(f, &ctx->mu[b], sizeof ctx->mu[b]) ||
            !read_exact(f, &ctx->radius[b], sizeof ctx->radius[b]) ||
            !read_exact(f, &ctx->flux[b], sizeof ctx->flux[b]) ||
            !read_exact(f, &has_orientation, sizeof has_orientation) ||
            !read_exact(f, &degree, sizeof degree)) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        ctx->names[b][EPH_NAME_SIZE - 1] = '\0';

        /* A body claiming orientation in a file whose header says there are
         * no orientation coefficients would send every read after it to the
         * wrong offset, so it is a corrupt file rather than a body to skip. */
        if (has_orientation > 1u ||
            (has_orientation == 1u && orient_degree == 0u)) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        ctx->orient_slot[b] = has_orientation ? (int)ctx->n_orient++ : -1;

        /* Negative is not "unknown", it is a corrupt file: unknown is zero,
         * and every reader of these two treats zero as "this body does not
         * occult" and "this body does not shine". A negative radius would
         * reach srp_shadow's own guard and be ignored there, and a negative
         * flux would pull a vessel toward the Sun. */
        if (ctx->radius[b] < 0.0 || ctx->flux[b] < 0.0) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        /* Degree 1 is not a point mass and not representable: the degree-1
         * terms vanish only in a frame centred on the body's centre of
         * mass, which is the frame this asset is in, so a file claiming
         * them is describing something this reader would get wrong.
         * Refused rather than truncated. */
        if (degree == 1u || degree > (unsigned)HARMONICS_MAX_DEGREE) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        ctx->harmonics[b].degree = (int)degree;

        if (degree >= 2u) {
            size_t n_terms = (size_t)(degree + 1u) * (degree + 2u) / 2u;

            if (!read_exact(f, &ctx->harmonics[b].re,
                            sizeof ctx->harmonics[b].re) ||
                !read_exact(f, ctx->harmonics[b].c,
                            n_terms * sizeof ctx->harmonics[b].c[0]) ||
                !read_exact(f, ctx->harmonics[b].s,
                            n_terms * sizeof ctx->harmonics[b].s[0])) {
                eph_free(ctx);
                fclose(f);
                return CORE_ERR_INVALID_ARG;
            }

            /* Read as they lie: v6 holds FULLY NORMALISED coefficients,
             * the same convention harmonics.c works in and the same one
             * published models arrive in (data/grail/README.md). v5 held
             * the unnormalised form and this function converted on load;
             * the version bump exists precisely so that no file can be read
             * under the wrong convention - the numbers differ by a factor
             * of sqrt(5) at degree 2, which is not a difference anything
             * would report as an error. */
            if (!(ctx->harmonics[b].re > 0.0)) {
                eph_free(ctx);
                fclose(f);
                return CORE_ERR_INVALID_ARG;
            }
        }

        /* The atmosphere (ROADMAP K7b). */
        unsigned n_layers;
        if (!read_exact(f, &n_layers, sizeof n_layers)) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        if (n_layers > (unsigned)ATMOSPHERE_MAX_LAYERS) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        ctx->atmosphere[b].n_layers = (int)n_layers;

        for (unsigned i = 0; i < n_layers; i++) {
            AtmosphereLayer *l = &ctx->atmosphere[b].layer[i];

            if (!read_exact(f, &l->base_altitude_m, sizeof l->base_altitude_m) ||
                !read_exact(f, &l->base_density, sizeof l->base_density) ||
                !read_exact(f, &l->scale_height_m, sizeof l->scale_height_m)) {
                eph_free(ctx);
                fclose(f);
                return CORE_ERR_INVALID_ARG;
            }

            /* Ascending and strictly, positive density, positive scale
             * height. atmosphere_density's band search assumes the first and
             * would return a silently wrong band without it; the other two it
             * survives, by treating the band as vacuum, which is a plausible
             * atmosphere built from a corrupt file and worse than a refusal.
             *
             * Checked here rather than there because this is where a file is
             * turned into a model. The runtime asks for a density thousands
             * of times a second and should not be re-validating a table that
             * was already accepted. */
            if (!(l->base_density > 0.0) || !(l->scale_height_m > 0.0) ||
                (i > 0 && !(l->base_altitude_m >
                            ctx->atmosphere[b].layer[i - 1].base_altitude_m))) {
                eph_free(ctx);
                fclose(f);
                return CORE_ERR_INVALID_ARG;
            }
        }
    }

    if (!read_exact(f, ctx->coeffs, n_coeffs * sizeof *ctx->coeffs)) {
        eph_free(ctx);
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    /* The orientation block (ROADMAP K3b) - one contiguous array after the
     * positions rather than interleaved per interval, so both are a single
     * read and the arithmetic that finds an interval is the same for both. */
    if (ctx->n_orient > 0) {
        size_t n_orient_coeffs = (size_t)n_intervals * ctx->n_orient * 4u
                               * orient_degree;

        ctx->orient = calloc(n_orient_coeffs, sizeof *ctx->orient);
        if (ctx->orient == NULL) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_BUFFER_TOO_SMALL;
        }
        if (!read_exact(f, ctx->orient,
                        n_orient_coeffs * sizeof *ctx->orient)) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
    }

    /* Trailing bytes mean the file is not what the header says it is. */
    char extra;
    if (fread(&extra, 1, 1, f) != 0) {
        eph_free(ctx);
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    fclose(f);
    *out = ctx;
    return CORE_OK;
}

void eph_free(EphemerisCtx *ctx)
{
    if (ctx == NULL) {
        return;
    }
    free(ctx->names);
    free(ctx->mu);
    free(ctx->radius);
    free(ctx->flux);
    free(ctx->coeffs);
    free(ctx->harmonics);
    free(ctx->atmosphere);
    free(ctx->orient_slot);
    free(ctx->orient);
    free(ctx);
}

int eph_body_count(const EphemerisCtx *ctx)
{
    return ctx == NULL ? 0 : (int)ctx->n_bodies;
}

const char *eph_body_name(const EphemerisCtx *ctx, int body)
{
    if (ctx == NULL || body < 0 || (unsigned)body >= ctx->n_bodies) {
        return NULL;
    }
    return ctx->names[body];
}

double eph_body_mu(const EphemerisCtx *ctx, int body)
{
    if (ctx == NULL || body < 0 || (unsigned)body >= ctx->n_bodies) {
        return 0.0;
    }
    return ctx->mu[body];
}

double eph_body_radius(const EphemerisCtx *ctx, int body)
{
    if (ctx == NULL || body < 0 || (unsigned)body >= ctx->n_bodies) {
        return 0.0;
    }
    return ctx->radius[body];
}

double eph_body_flux(const EphemerisCtx *ctx, int body)
{
    if (ctx == NULL || body < 0 || (unsigned)body >= ctx->n_bodies) {
        return 0.0;
    }
    return ctx->flux[body];
}

const HarmonicsField *eph_body_harmonics(const EphemerisCtx *ctx, int body)
{
    if (ctx == NULL || body < 0 || (unsigned)body >= ctx->n_bodies) {
        return NULL;
    }

    return &ctx->harmonics[body];
}

CoreResult eph_body_atmosphere(const EphemerisCtx *ctx, int body,
                               AtmosphereModel *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

    *out = ctx->atmosphere[body];
    return CORE_OK;
}

CoreResult eph_span(const EphemerisCtx *ctx, double *t_begin, double *t_end)
{
    if (ctx == NULL || t_begin == NULL || t_end == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    *t_begin = ctx->t_begin;
    *t_end = ctx->t_begin + (double)ctx->n_intervals * ctx->interval;
    return CORE_OK;
}

/* The interval containing t, and its two ends. Shared by position and
 * orientation so the two cannot disagree about which polynomial covers an
 * instant - they are fitted over the same intervals, and finding them twice
 * would be two chances to round differently. */
static CoreResult interval_of(const EphemerisCtx *ctx, double t,
                              long *index_out, double *a_out, double *b_out)
{
    double offset = t - ctx->t_begin;
    double span = (double)ctx->n_intervals * ctx->interval;

    if (!(offset >= 0.0) || offset > span) {
        return CORE_ERR_INVALID_ARG;
    }

    /* Truncation, not rounding: the interval containing t. The last epoch
     * lands exactly on the end of the span, where the division gives the
     * interval count and there is no such interval; it belongs to the last
     * one. */
    long index = (long)(offset / ctx->interval);
    if (index >= (long)ctx->n_intervals) {
        index = (long)ctx->n_intervals - 1;
    }

    *index_out = index;
    *a_out = ctx->t_begin + (double)index * ctx->interval;
    *b_out = *a_out + ctx->interval;
    return CORE_OK;
}

CoreResult eph_body_orientation(const EphemerisCtx *ctx, int body, double t,
                                Quat *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

    long index;
    double a, b;
    if (interval_of(ctx, t, &index, &a, &b) != CORE_OK) {
        return CORE_ERR_INVALID_ARG;
    }

    /* Not modelled reads back as no rotation at all, exactly - see the
     * header. Checked after the time, so that "outside the span" stays an
     * error for every body rather than only for the ones that rotate. */
    if (ctx->orient_slot[body] < 0) {
        *out = quat_identity();
        return CORE_OK;
    }

    size_t stride = (size_t)ctx->orient_degree;
    size_t base = ((size_t)index * ctx->n_orient
                   + (size_t)ctx->orient_slot[body]) * 4u * stride;

    const double *cw = ctx->orient + base;
    const double *cx = cw + stride;
    const double *cy = cx + stride;
    const double *cz = cy + stride;

    Quat q = { cheb_eval(cw, stride, a, b, t),
               cheb_eval(cx, stride, a, b, t),
               cheb_eval(cy, stride, a, b, t),
               cheb_eval(cz, stride, a, b, t) };

    /* The fit is unit length at its nodes and only nearly so between them,
     * so this is not defensive: it is the one invariant the four channels
     * left to restore, and restoring it is a sqrt. */
    *out = quat_normalize(q);
    return CORE_OK;
}

CoreResult eph_body_angular_velocity(const EphemerisCtx *ctx, int body,
                                     double t, Vec3d *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

    long index;
    double a, b;
    if (interval_of(ctx, t, &index, &a, &b) != CORE_OK) {
        return CORE_ERR_INVALID_ARG;
    }

    /* A body whose rotation is not modelled does not turn. The same answer
     * eph_body_orientation gives, and the same kind of answer: "not
     * modelled", not "failed". */
    if (ctx->orient_slot[body] < 0) {
        *out = vec3_zero();
        return CORE_OK;
    }

    size_t stride = (size_t)ctx->orient_degree;
    size_t base = ((size_t)index * ctx->n_orient
                   + (size_t)ctx->orient_slot[body]) * 4u * stride;

    const double *cw = ctx->orient + base;
    const double *cx = cw + stride;
    const double *cy = cx + stride;
    const double *cz = cy + stride;

    Quat q = { cheb_eval(cw, stride, a, b, t),
               cheb_eval(cx, stride, a, b, t),
               cheb_eval(cy, stride, a, b, t),
               cheb_eval(cz, stride, a, b, t) };

    /* The analytic derivative of the same polynomial, exactly as the velocity
     * of a body is the derivative of its position fit (cheb_eval_deriv). Not
     * a finite difference: differencing a quaternion whose own fit error is
     * 1e-13 over a step small enough to be local would be all error. */
    Quat qd = { cheb_eval_deriv(cw, stride, a, b, t),
                cheb_eval_deriv(cx, stride, a, b, t),
                cheb_eval_deriv(cy, stride, a, b, t),
                cheb_eval_deriv(cz, stride, a, b, t) };

    /* omega = 2 * vector part of (qd (x) q*), which is the inertial-frame
     * angular velocity for q taking body-fixed components to inertial ones -
     * quat.h's convention, and the one eph_body_orientation returns.
     *
     * Written out rather than built from a general quaternion product: only
     * the vector part is wanted, the scalar part is identically zero for a
     * unit q, and computing it to discard it would invite someone to check
     * whether it really is zero and find 1e-17.
     *
     * NOT normalised first. q from the fit is unit to about 1e-13, and
     * dividing both q and qd by |q| would change omega by the same 1e-13
     * while costing a sqrt and a division per evaluation - this sits in the
     * force loop. The error it saves is four orders below the fit error the
     * orientation channels already carry. */
    out->x = 2.0 * (-qd.w * q.x + qd.x * q.w - qd.y * q.z + qd.z * q.y);
    out->y = 2.0 * (-qd.w * q.y + qd.x * q.z + qd.y * q.w - qd.z * q.x);
    out->z = 2.0 * (-qd.w * q.z - qd.x * q.y + qd.y * q.x + qd.z * q.w);
    return CORE_OK;
}

CoreResult eph_body_state(const EphemerisCtx *ctx, int body, double t,
                          State *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

    long index;
    double a, b;
    if (interval_of(ctx, t, &index, &a, &b) != CORE_OK) {
        return CORE_ERR_INVALID_ARG;
    }

    size_t stride = (size_t)ctx->degree;
    size_t base = ((size_t)index * ctx->n_bodies + (size_t)body) * 3u * stride;

    const double *cx = ctx->coeffs + base;
    const double *cy = cx + stride;
    const double *cz = cy + stride;

    out->r = vec3(cheb_eval(cx, stride, a, b, t),
                  cheb_eval(cy, stride, a, b, t),
                  cheb_eval(cz, stride, a, b, t));

    out->v = vec3(cheb_eval_deriv(cx, stride, a, b, t),
                  cheb_eval_deriv(cy, stride, a, b, t),
                  cheb_eval_deriv(cz, stride, a, b, t));

    out->t = t;
    return CORE_OK;
}
