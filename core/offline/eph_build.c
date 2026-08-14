#include "eph_build.h"

#include "cheb.h"
#include "cheb_fit.h"
#include "ephemeris.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Samples per interval are the fit nodes themselves, so this bounds the
 * degree as well. */
#define MAX_DEGREE CHEB_FIT_MAX_N

/* Default on. The Makefile passes -DEPH_ANCHOR_BARYCENTRE=0 when asked; this
 * fallback is what a build outside the Makefile gets, and it should be what
 * ships. See eph_anchor_enabled() in the header for why it is a build switch. */
#ifndef EPH_ANCHOR_BARYCENTRE
#define EPH_ANCHOR_BARYCENTRE 1
#endif

int eph_anchor_enabled(void)
{
    return EPH_ANCHOR_BARYCENTRE;
}

static int write_exact(FILE *f, const void *src, size_t n)
{
    return fwrite(src, 1, n, f) == n;
}

CoreResult eph_build(const NBodySystem *sys, const State *initial,
                     const EphBodyInfo *bodies,
                     const EphBuildConfig *cfg,
                     const char *out_path,
                     EphBuildReport *report)
{
    if (sys == NULL || initial == NULL || bodies == NULL || cfg == NULL ||
        out_path == NULL) {
        return CORE_ERR_INVALID_ARG;
    }
    if (sys->n == 0 || sys->n > NBODY_MAX) {
        return CORE_ERR_INVALID_ARG;
    }
    if (cfg->degree < 2 || cfg->degree > MAX_DEGREE) {
        return CORE_ERR_INVALID_ARG;
    }
    if (!(cfg->interval_seconds > 0.0) || !(cfg->t_end > cfg->t_begin) ||
        !(cfg->tol_m > 0.0)) {
        return CORE_ERR_INVALID_ARG;
    }

    size_t n_bodies = sys->n;
    size_t degree = cfg->degree;

    double span = cfg->t_end - cfg->t_begin;
    size_t n_intervals = (size_t)(span / cfg->interval_seconds);
    if ((double)n_intervals * cfg->interval_seconds < span) {
        n_intervals++;   /* cover the tail rather than leave a gap */
    }
    if (n_intervals == 0) {
        return CORE_ERR_INVALID_ARG;
    }

    FILE *f = fopen(out_path, "wb");
    if (f == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    unsigned version = EPH_VERSION;
    unsigned u_bodies = (unsigned)n_bodies;
    unsigned u_intervals = (unsigned)n_intervals;
    unsigned u_degree = (unsigned)degree;
    double sentinel = 1.0;

    int ok =
        write_exact(f, EPH_MAGIC, EPH_MAGIC_SIZE) &&
        write_exact(f, &version, sizeof version) &&
        write_exact(f, &u_bodies, sizeof u_bodies) &&
        write_exact(f, &u_intervals, sizeof u_intervals) &&
        write_exact(f, &u_degree, sizeof u_degree) &&
        write_exact(f, &cfg->t_begin, sizeof cfg->t_begin) &&
        write_exact(f, &cfg->interval_seconds, sizeof cfg->interval_seconds) &&
        write_exact(f, &sentinel, sizeof sentinel);

    for (size_t b = 0; ok && b < n_bodies; b++) {
        char name[EPH_NAME_SIZE];
        memset(name, 0, sizeof name);
        snprintf(name, sizeof name, "%s", bodies[b].name);

        /* Straight from the system being integrated, never from a second
         * source (ROADMAP K4b). That is the whole point of putting these in
         * the asset: whatever shape the cooker moved the bodies under is
         * the shape a vessel reading this file will fly in, and there is no
         * arrangement of the code in which the two can differ. */
        int has = sys->has_j2 && (size_t)sys->j2_body == b;
        unsigned degree = has ? (unsigned)sys->j2_field.degree : 0u;

        ok = write_exact(f, name, sizeof name) &&
             write_exact(f, &sys->mu[b], sizeof sys->mu[b]) &&
             write_exact(f, &bodies[b].radius_m, sizeof bodies[b].radius_m) &&
             write_exact(f, &bodies[b].flux_1au, sizeof bodies[b].flux_1au) &&
             write_exact(f, &degree, sizeof degree);

        if (ok && degree >= 2u) {
            size_t n_terms = (size_t)(degree + 1u) * (degree + 2u) / 2u;
            ok = write_exact(f, &sys->j2_field.re, sizeof sys->j2_field.re) &&
                 write_exact(f, sys->j2_field.c,
                             n_terms * sizeof sys->j2_field.c[0]) &&
                 write_exact(f, sys->j2_field.s,
                             n_terms * sizeof sys->j2_field.s[0]);
        }
    }

    if (!ok) {
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    Dop853Config integ;
    memset(&integ, 0, sizeof integ);
    integ.tol_m = cfg->tol_m;
    integ.max_steps = 50000000;

    Dop853State st;
    memset(&st, 0, sizeof st);

    State current[NBODY_MAX];
    for (size_t b = 0; b < n_bodies; b++) {
        current[b] = initial[b];
    }

    /* On the working copy, so the caller's initial conditions stay the
     * published ones. Before the first node, so every interval sees it. */
    if (eph_anchor_enabled()) {
        nbody_anchor_barycentre(sys, current);
    }

    double max_fit_error = 0.0;

    for (size_t interval = 0; ok && interval < n_intervals; interval++) {
        double a = cfg->t_begin + (double)interval * cfg->interval_seconds;
        double b_end = a + cfg->interval_seconds;

        double nodes[MAX_DEGREE];
        CoreResult r = cheb_nodes(a, b_end, nodes, degree);
        if (r != CORE_OK) {
            fclose(f);
            return r;
        }

        /* Node times descend, because the nodes are cosines, so the forward
         * march visits them in reverse index order.
         *
         * The accuracy probe sits midway between the two central nodes - the
         * least constrained point of the interval - and is visited in the
         * same forward pass. Integrating back to it afterwards would work and
         * would also double the cost of the whole cook. */
        double samples[NBODY_MAX][3][MAX_DEGREE];
        double probe = 0.5 * (nodes[degree / 2] + nodes[degree / 2 - 1]);
        State probe_state[NBODY_MAX];
        int have_probe = 0;

        for (size_t k = degree; k-- > 0; ) {
            State next[NBODY_MAX];
            r = nbody_integrate(sys, current, nodes[k], &integ, &st, next);
            if (r != CORE_OK) {
                fclose(f);
                return r;
            }
            memcpy(current, next, sizeof next);

            for (size_t body = 0; body < n_bodies; body++) {
                samples[body][0][k] = current[body].r.x;
                samples[body][1][k] = current[body].r.y;
                samples[body][2][k] = current[body].r.z;
            }

            if (k == degree / 2) {
                r = nbody_integrate(sys, current, probe, &integ, &st, next);
                if (r != CORE_OK) {
                    fclose(f);
                    return r;
                }
                memcpy(current, next, sizeof next);
                memcpy(probe_state, current, sizeof probe_state);
                have_probe = 1;
            }
        }

        /* Fit, then measure the fit where it was not constrained. The nodes
         * themselves are reproduced exactly by construction, so checking
         * there would measure nothing at all. */
        double coeffs[NBODY_MAX][3][MAX_DEGREE];

        for (size_t body = 0; ok && body < n_bodies; body++) {
            for (int c = 0; c < 3; c++) {
                r = cheb_fit_samples(samples[body][c], coeffs[body][c], degree);
                if (r != CORE_OK) {
                    fclose(f);
                    return r;
                }
                ok = write_exact(f, coeffs[body][c],
                                 degree * sizeof coeffs[body][c][0]);
            }
        }

        if (have_probe) {
            for (size_t body = 0; body < n_bodies; body++) {
                Vec3d fitted = vec3(
                    cheb_eval(coeffs[body][0], degree, a, b_end, probe),
                    cheb_eval(coeffs[body][1], degree, a, b_end, probe),
                    cheb_eval(coeffs[body][2], degree, a, b_end, probe));

                double e = vec3_distance(fitted, probe_state[body].r);
                if (e > max_fit_error) {
                    max_fit_error = e;
                }
            }
        }

        /* Advance to the end of the interval so the next one starts where it
         * should. The probe above ran on a copy and left nothing behind. */
        State next[NBODY_MAX];
        r = nbody_integrate(sys, current, b_end, &integ, &st, next);
        if (r != CORE_OK) {
            fclose(f);
            return r;
        }
        memcpy(current, next, sizeof next);
    }

    long position = ftell(f);
    int closed_ok = fclose(f) == 0;

    if (!ok || !closed_ok || position < 0) {
        return CORE_ERR_INVALID_ARG;
    }

    if (report != NULL) {
        report->integrator_steps = st.n_accepted;
        report->intervals = n_intervals;
        report->bytes_written = (size_t)position;
        report->max_fit_error_m = max_fit_error;
    }

    return CORE_OK;
}
