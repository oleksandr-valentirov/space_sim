#include "ephemeris.h"

#include "cheb.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct EphemerisCtx {
    unsigned n_bodies;
    unsigned n_intervals;
    unsigned degree;

    double t_begin;
    double interval;

    char   (*names)[EPH_NAME_SIZE];
    double  *mu;
    double  *radius;   /* metres, 0 where the asset does not say */
    double  *flux;     /* W/m^2 at 1 AU, 0 for a body that does not shine */
    double  *coeffs;   /* [interval][body][component][degree] */

    /* One per body, degree 0 where the asset says point mass. Stored
     * expanded rather than as the file's variable-length blocks: the file
     * is read once, and a reader that had to walk a triangular array to
     * answer eph_body_harmonics would be trading a few kilobytes for a
     * bug. */
    HarmonicsField *harmonics;
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
    unsigned version, n_bodies, n_intervals, degree;
    double t_begin, interval, sentinel;

    int header_ok =
        read_exact(f, magic, sizeof magic) &&
        memcmp(magic, EPH_MAGIC, sizeof magic) == 0 &&
        read_exact(f, &version, sizeof version) &&
        version == EPH_VERSION &&
        read_exact(f, &n_bodies, sizeof n_bodies) &&
        read_exact(f, &n_intervals, sizeof n_intervals) &&
        read_exact(f, &degree, sizeof degree) &&
        read_exact(f, &t_begin, sizeof t_begin) &&
        read_exact(f, &interval, sizeof interval) &&
        read_exact(f, &sentinel, sizeof sentinel);

    /* The sentinel is exactly 1.0 written as a double. If it does not read
     * back as 1.0, the file came from a machine that disagrees about byte
     * order or floating point layout, and every number after it is noise. */
    if (!header_ok || sentinel != 1.0 ||
        n_bodies == 0 || n_intervals == 0 || degree < 2 ||
        !(interval > 0.0)) {
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
    ctx->t_begin = t_begin;
    ctx->interval = interval;

    size_t n_coeffs = (size_t)n_intervals * n_bodies * 3u * degree;

    ctx->names = calloc(n_bodies, sizeof *ctx->names);
    ctx->mu = calloc(n_bodies, sizeof *ctx->mu);
    ctx->radius = calloc(n_bodies, sizeof *ctx->radius);
    ctx->flux = calloc(n_bodies, sizeof *ctx->flux);
    ctx->coeffs = calloc(n_coeffs, sizeof *ctx->coeffs);
    ctx->harmonics = calloc(n_bodies, sizeof *ctx->harmonics);

    if (ctx->names == NULL || ctx->mu == NULL || ctx->radius == NULL ||
        ctx->flux == NULL || ctx->coeffs == NULL ||
        ctx->harmonics == NULL) {
        eph_free(ctx);
        fclose(f);
        return CORE_ERR_BUFFER_TOO_SMALL;
    }

    for (unsigned b = 0; b < n_bodies; b++) {
        unsigned degree;

        if (!read_exact(f, ctx->names[b], EPH_NAME_SIZE) ||
            !read_exact(f, &ctx->mu[b], sizeof ctx->mu[b]) ||
            !read_exact(f, &ctx->radius[b], sizeof ctx->radius[b]) ||
            !read_exact(f, &ctx->flux[b], sizeof ctx->flux[b]) ||
            !read_exact(f, &degree, sizeof degree)) {
            eph_free(ctx);
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        ctx->names[b][EPH_NAME_SIZE - 1] = '\0';

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

            if (!(ctx->harmonics[b].re > 0.0)) {
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

CoreResult eph_body_harmonics(const EphemerisCtx *ctx, int body,
                              HarmonicsField *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

    *out = ctx->harmonics[body];
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

CoreResult eph_body_state(const EphemerisCtx *ctx, int body, double t,
                          State *out)
{
    if (ctx == NULL || out == NULL || body < 0 ||
        (unsigned)body >= ctx->n_bodies) {
        return CORE_ERR_INVALID_ARG;
    }

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

    double a = ctx->t_begin + (double)index * ctx->interval;
    double b = a + ctx->interval;

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
