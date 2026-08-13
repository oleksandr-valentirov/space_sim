/* Propagating a vessel (ROADMAP H1, PROJECT.md section 5).
 *
 * This is the runtime path the game flies on, and the second half of the C API
 * the Rust side needs: eph_* says where the bodies are, prop_* moves a vessel
 * through the field they make. Everything under it already existed and is
 * already tested - field_all_bodies for the ten point masses, accel_field for
 * the sum, dop853_integrate for the steps. What is new here is the shape the
 * boundary needs, and one property that has to be true rather than hoped for:
 *
 *     A prediction cut into pieces is bit-identical to one uninterrupted run.
 *
 * That is CLAUDE.md invariant 5 - prediction and physics are one integrator
 * and one tolerance, and a computed stretch of the prediction becomes history
 * rather than being integrated a second time. A flight planner that draws a
 * line the vessel then does not fly is not a bug in the drawing.
 *
 * ---
 *
 * Three decisions worth stating, because each had a plausible alternative.
 *
 * 1. Samples are the integrator's accepted steps, not a uniform grid.
 *
 *    Sampling on chosen times means landing steps on them, and landing steps
 *    on chosen times is not free: it takes the step sequence away from the
 *    error controller. The ephemeris cooker measured how far that goes - with
 *    forced landings on fit nodes, changing the tolerance by two orders of
 *    magnitude changed nothing at all, because the nodes, not the tolerance,
 *    were setting the step (ROADMAP, "Допуск оновлено: 1 м -> 1e-6 м").
 *
 *    The adaptive step is also the better sampling for the thing these points
 *    are for. It shortens where the trajectory curves and stretches where it
 *    does not, which is exactly where a polyline needs vertices and where it
 *    does not.
 *
 * 2. The trajectory does not live in the context.
 *
 *    prop_run takes the initial state and returns the final one, and the step
 *    size travels in and out through in_out_step. So the context holds only
 *    what a configuration holds, and the state of a vessel is the caller's -
 *    which is what PROJECT.md section 4 requires of a save anyway: state plus
 *    manoeuvre plan plus the integrator's step, not the trajectory.
 *
 * 3. A full buffer is a reason to stop, not an error.
 *
 *    CORE_ERR_BUFFER_TOO_SMALL would be wrong here: nothing failed, the run
 *    reached a point the caller can continue from. The same reasoning as the
 *    skipped cells of a porkchop grid (core/planning/porkchop.h) - an expected
 *    outcome reported as data.
 *
 * What is deliberately not here yet: VesselParams (mass, area, cr, cd) from
 * the section 5 sketch. Nothing reads them until SRP and drag arrive at M3.5,
 * and a struct whose every field is ignored is worse than its absence - the
 * caller fills it in, nothing happens, and nothing says so. A vessel is a
 * massless test particle here, which is not an approximation waiting to be
 * improved but the split the architecture rests on (core/field.h). */

#ifndef CORE_PROP_H
#define CORE_PROP_H

#include "core.h"
#include "ephemeris.h"

#include <stddef.h>

typedef struct PropagatorCtx PropagatorCtx;

/* PROJECT.md section 4 asks for this field from the first day, so that adding
 * RKN later is a choice at a call site rather than a rewrite. Any value but
 * DOP853 is CORE_ERR_INVALID_ARG today, which is the honest report: the field
 * exists, the second integrator does not. */
typedef enum {
    CORE_INTEG_DOP853 = 0,
    CORE_INTEG_RKN    = 1, /* M3.5 or later, if the profiler asks for it */
} CoreIntegrator;

typedef struct {
    CoreIntegrator integrator;

    /* Absolute position tolerance in metres, passed straight to the
     * integrator. One number, and it is the same one the prediction and the
     * flown trajectory both use - there is no second tolerance to disagree
     * with it. */
    double tol_m;

    /* Ceiling on the step, seconds. 0 means "the integrator picks one", and
     * what it picks is the length of the leg it was given (core/dop853.c).
     *
     * Set it. With 0 the ceiling is leg-dependent, and a stitched run gets a
     * different one on its last leg than an uninterrupted run ever sees.
     * Measured (core/test/test_prop.c, two days of a geostationary orbit at a
     * centimetre): the trajectory still comes out bit-identical, and the step
     * left behind in in_out_step does not - 5493.85 s against 6440.34 s. That
     * is the half that does not show in the answer, and it is what the next
     * call continues with. With a real ceiling both match to the bit. */
    double h_max_s;

    /* Steps allowed per prop_run call, not per trajectory. 0 -> integrator
     * default. Exceeding it is CORE_ERR_TOLERANCE_NOT_MET. */
    long max_steps;
} PropConfig;

/* Why a run ended. Not an error code: every value here is a normal outcome. */
typedef enum {
    /* Reached t_end. */
    CORE_STOP_T_END = 0,
    /* out_states filled up first. Continue from out_final with the same
     * in_out_step; the continuation is the same trajectory. */
    CORE_STOP_BUFFER_FULL = 1,
} CoreStopReason;

/* The context borrows the ephemeris and does not own it: it must outlive every
 * propagator built on it. On the Rust side that is not a promise but a type -
 * the wrapper holds an Arc (ROADMAP H4).
 *
 * Allocating pair, the documented exception to "C allocates no buffers"
 * (PROJECT.md section 5, rule 1). prop_free(NULL) is allowed, which is what
 * lets a Drop implementation free unconditionally. */
CoreResult prop_create(const EphemerisCtx *eph, const PropConfig *cfg,
                       PropagatorCtx **out);
void       prop_free(PropagatorCtx *p);

/* Integrate from *initial to t_end, in the field of every body in the asset.
 *
 * Fills out_states with the state at the end of each accepted step and stops
 * early if it runs out of room. Pass out_states = NULL (and out_cap = 0) to
 * propagate without sampling: that is the same integration, step for step, and
 * the test that says so is the reason the flight planner and the vessel can
 * share one code path. An empty non-NULL buffer is CORE_ERR_INVALID_ARG rather
 * than an immediate stop, because a caller stitching legs would make no
 * progress and never find out why.
 *
 * The initial state is not sampled - the caller already has it. So stitched
 * legs concatenate into a polyline with no repeated vertices.
 *
 * in_out_step carries the integrator's step: 0 on the first call ("choose
 * one"), and afterwards whatever the previous call left there. It is the piece
 * PROJECT.md section 4 requires in the save, and passing a fresh 0 instead of
 * the carried value produces a different trajectory - measurably, which is how
 * the test knows the carry is real.
 *
 * Errors, and both are worth being loud about:
 *
 *   CORE_ERR_INVALID_ARG        a time outside the ephemeris span was needed.
 *                               The field returns zero acceleration there and
 *                               sets a sticky flag (core/field.h); without
 *                               this check the result would be a plausible
 *                               trajectory of a vessel that felt no gravity.
 *   CORE_ERR_TOLERANCE_NOT_MET  the step controller gave up: max_steps, or a
 *                               step driven below h_min, or a state gone
 *                               non-finite (a NaN error norm is never < 1, so
 *                               every step is rejected and the run ends here
 *                               rather than returning nonsense).
 *
 * out_count, out_final, out_stop and in_out_step are all required; out_states
 * is the only optional one. */
CoreResult prop_run(PropagatorCtx *p, const State *initial, double t_end,
                    State *out_states, size_t out_cap, size_t *out_count,
                    State *out_final, CoreStopReason *out_stop,
                    double *in_out_step);

#endif /* CORE_PROP_H */
