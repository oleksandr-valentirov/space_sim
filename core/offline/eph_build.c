#include "eph_build.h"

#include "body_rotation.h"
#include "cheb.h"
#include "cheb_fit.h"
#include "ephemeris.h"
#include "quat.h"

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

/* q and -q are the same rotation, and mat3_to_quat picks between them by
 * whichever diagonal entry happens to be largest, so a sequence of samples
 * of a turning body flips sign wherever that branch changes. Four channels
 * that jump between +q and -q are not a smooth function and cannot be
 * fitted at all, so every sample is brought to the same side as the one
 * before it.
 *
 * This works while consecutive samples are less than half a turn of the
 * quaternion apart, which is a quarter turn of the body. Earth's widest
 * node gap on the fixture's interval is about 63 degrees of quaternion, so
 * there is margin, but not unbounded margin - and the same lengthening of
 * the interval that would break the fit breaks this first. Both are caught
 * by the same number, max_orient_error_rad. */
static Quat same_side_as(Quat q, Quat previous)
{
    double dot = q.w * previous.w + q.x * previous.x +
                 q.y * previous.y + q.z * previous.z;
    if (dot < 0.0) {
        Quat flipped = { -q.w, -q.x, -q.y, -q.z };
        return flipped;
    }
    return q;
}

static double quat_max_component_diff(Quat a, Quat b)
{
    double d[4] = { a.w - b.w, a.x - b.x, a.y - b.y, a.z - b.z };
    double worst = 0.0;

    for (int i = 0; i < 4; i++) {
        double m = d[i] < 0.0 ? -d[i] : d[i];
        if (m > worst) {
            worst = m;
        }
    }
    return worst;
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
    if (cfg->orient_degree == 1 || cfg->orient_degree > MAX_DEGREE) {
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

    /* A body gets orientation channels only if there is a model to fill them
     * from; the rest read back as the identity without costing a byte. So
     * the header's orientation degree is zero unless someone is using it,
     * and an asset of nothing but unmodelled bodies is byte for byte what
     * this cooker wrote before K3b except for the version. */
    size_t n_orient = 0;
    for (size_t b = 0; b < n_bodies; b++) {
        if (cfg->orient_degree > 0 && body_rotation_has_model(bodies[b].name)) {
            n_orient++;
        }
    }
    size_t orient_degree = n_orient > 0 ? cfg->orient_degree : 0;

    unsigned version = EPH_VERSION;
    unsigned u_bodies = (unsigned)n_bodies;
    unsigned u_intervals = (unsigned)n_intervals;
    unsigned u_degree = (unsigned)degree;
    unsigned u_orient_degree = (unsigned)orient_degree;
    double sentinel = 1.0;

    int ok =
        write_exact(f, EPH_MAGIC, EPH_MAGIC_SIZE) &&
        write_exact(f, &version, sizeof version) &&
        write_exact(f, &u_bodies, sizeof u_bodies) &&
        write_exact(f, &u_intervals, sizeof u_intervals) &&
        write_exact(f, &u_degree, sizeof u_degree) &&
        write_exact(f, &u_orient_degree, sizeof u_orient_degree) &&
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
        unsigned has_orientation =
            orient_degree > 0 && body_rotation_has_model(bodies[b].name);

        ok = write_exact(f, name, sizeof name) &&
             write_exact(f, &sys->mu[b], sizeof sys->mu[b]) &&
             write_exact(f, &bodies[b].radius_m, sizeof bodies[b].radius_m) &&
             write_exact(f, &bodies[b].flux_1au, sizeof bodies[b].flux_1au) &&
             write_exact(f, &has_orientation, sizeof has_orientation) &&
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

    /* Orientation (ROADMAP K3b), in its own pass after the positions rather
     * than interleaved with them, because that is the layout the reader
     * wants: two contiguous blocks, one read each. It costs nothing to run
     * separately - body_rotation_of is a closed-form function of t, not
     * something the integrator has to march to. */
    double max_orient_error = 0.0;

    if (orient_degree > 0) {
        Quat previous[NBODY_MAX];
        int have_previous[NBODY_MAX];
        memset(have_previous, 0, sizeof have_previous);

        for (size_t interval = 0; ok && interval < n_intervals; interval++) {
            double a = cfg->t_begin + (double)interval * cfg->interval_seconds;
            double b_end = a + cfg->interval_seconds;

            double nodes[MAX_DEGREE];
            CoreResult r = cheb_nodes(a, b_end, nodes, orient_degree);
            if (r != CORE_OK) {
                fclose(f);
                return r;
            }

            /* Same least constrained point as the position probe: midway
             * between the two central nodes. */
            double probe = 0.5 * (nodes[orient_degree / 2]
                                  + nodes[orient_degree / 2 - 1]);

            for (size_t body = 0; ok && body < n_bodies; body++) {
                if (!body_rotation_has_model(bodies[body].name)) {
                    continue;
                }

                double samples[4][MAX_DEGREE], coeffs[4][MAX_DEGREE];

                /* Nodes descend in time, so this walks them forward, and the
                 * sign chain runs forward with it - across intervals too,
                 * seeded from the last sample of the previous one. */
                for (size_t k = orient_degree; k-- > 0; ) {
                    Quat q;
                    r = body_rotation_of(bodies[body].name, nodes[k], &q);
                    if (r != CORE_OK) {
                        fclose(f);
                        return r;
                    }
                    if (have_previous[body]) {
                        q = same_side_as(q, previous[body]);
                    }
                    previous[body] = q;
                    have_previous[body] = 1;

                    samples[0][k] = q.w;
                    samples[1][k] = q.x;
                    samples[2][k] = q.y;
                    samples[3][k] = q.z;
                }

                for (int c = 0; ok && c < 4; c++) {
                    r = cheb_fit_samples(samples[c], coeffs[c], orient_degree);
                    if (r != CORE_OK) {
                        fclose(f);
                        return r;
                    }
                    ok = write_exact(f, coeffs[c],
                                     orient_degree * sizeof coeffs[c][0]);
                }

                /* What the runtime will actually get back: evaluated off the
                 * nodes and renormalised, exactly as eph_body_orientation
                 * does it. Comparing the raw fit instead would report a
                 * defect the reader removes. */
                Quat fitted = {
                    cheb_eval(coeffs[0], orient_degree, a, b_end, probe),
                    cheb_eval(coeffs[1], orient_degree, a, b_end, probe),
                    cheb_eval(coeffs[2], orient_degree, a, b_end, probe),
                    cheb_eval(coeffs[3], orient_degree, a, b_end, probe)
                };
                fitted = quat_normalize(fitted);

                Quat truth;
                r = body_rotation_of(bodies[body].name, probe, &truth);
                if (r != CORE_OK) {
                    fclose(f);
                    return r;
                }
                truth = same_side_as(truth, fitted);

                /* A component difference d corresponds to at most 2d radians
                 * of rotation - see EphBuildReport::max_orient_error_rad. */
                double e = 2.0 * quat_max_component_diff(fitted, truth);
                if (e > max_orient_error) {
                    max_orient_error = e;
                }
            }
        }
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
        report->max_orient_error_rad = max_orient_error;
    }

    return CORE_OK;
}
