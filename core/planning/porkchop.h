/* Porkchop-plot grid: the UI for choosing a transfer window (PROJECT.md
 * section 8, "Flight planner" - "porkchop-плоти як UI для вибору вікна").
 *
 * Same zone as lambert.h: outside the determinism boundary, libm allowed,
 * part of core/planning rather than core/ proper (PROJECT.md section 4,
 * ROADMAP.md G1). This module is the grid sweep built directly on top of
 * lambert_solve; nothing here is new physics. */

#ifndef CORE_PLANNING_PORKCHOP_H
#define CORE_PLANNING_PORKCHOP_H

#include "core.h"

#include <stddef.h>

/* A departure or arrival body's state at time t. Decouples this module from
 * any one source of ephemeris - the loaded asset (core/ephemeris.h), a
 * synthetic orbit, a test fixture - the same way AccelFunc (core/accel.h)
 * decouples the integrator from any one force model. */
typedef CoreResult (*BodyStateFunc)(double t, void *ctx, Vec3d *r, Vec3d *v);

typedef struct {
    double t1;             /* departure epoch */
    double tof;             /* time of flight; arrival epoch is t1 + tof */
    double v_inf_depart;   /* m/s: |transfer velocity - departure body's velocity| at t1 */
    double v_inf_arrive;   /* m/s: |transfer velocity - arrival body's velocity| at t1 + tof */
} PorkchopPoint;

/* Sweep every (t1, tof) pair in t1_grid x tof_grid - n_t1 * n_tof cells -
 * solving Lambert's problem for each and recording the hyperbolic excess
 * speed at both ends.
 *
 * Patched-conic, not corrected against the full ephemeris: that refinement
 * (differential correction / multiple shooting, PROJECT.md section 8) is a
 * later step applied to the one trajectory the player picks, not to every
 * cell of a grid that can have thousands. Zero-revolution only, same as
 * lambert_solve.
 *
 * A cell whose Lambert solve fails - degenerate geometry, or a time of
 * flight the zero-revolution solver cannot reach (core/planning/lambert.h)
 * - is skipped, not reported as an error: a porkchop grid always has
 * forbidden regions, and the plot is exactly the tool for seeing where they
 * are. depart or arrive returning anything but CORE_OK for a given epoch
 * (e.g. outside a loaded ephemeris's span) skips that epoch's whole row or
 * column the same way.
 *
 * Returns CORE_ERR_BUFFER_TOO_SMALL if the grid has more valid cells than
 * out_cap fits; out_count is set to how many were written either way, same
 * convention as core/refdata.h. Returns CORE_ERR_INVALID_ARG for a NULL
 * pointer, an empty grid, or mu <= 0 - checked before either BodyStateFunc
 * is called. */
CoreResult porkchop_compute(BodyStateFunc depart, void *depart_ctx,
                            BodyStateFunc arrive, void *arrive_ctx,
                            double mu, int prograde,
                            const double *t1_grid, size_t n_t1,
                            const double *tof_grid, size_t n_tof,
                            PorkchopPoint *out, size_t out_cap,
                            size_t *out_count);

#endif /* CORE_PLANNING_PORKCHOP_H */
