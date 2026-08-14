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
 * VesselParams (core/core.h) arrived with K6b and travels per run rather than
 * per context, exactly as the section 5 sketch had it. Two reasons, and the
 * second is the one that decides:
 *
 *   - mass changes when fuel burns, and a manoeuvre is a leg boundary, so a
 *     per-context vessel would mean rebuilding the propagator at every burn;
 *   - the game owns ONE propagator and flies every vessel through it
 *     (game/src/world.rs). A vessel in the configuration would make that a
 *     single spacecraft with several trajectories.
 *
 * It may be NULL, and a NULL vessel is a massless test particle in the field
 * of the bodies - which is not an approximation waiting to be improved but
 * the split the architecture rests on (core/field.h) for everything except
 * sunlight. That path is bit-for-bit what this file did before K6b. */

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
    /* An event fired. out_final is the state at the event, and out_event says
     * which one it was. */
    CORE_STOP_EVENT = 2,
} CoreStopReason;

/* ---- Events (ROADMAP H2) ------------------------------------------------ *
 *
 * Described by data, and the root finding lives in C. That pairing is what
 * makes CLAUDE.md invariant 7 - no callbacks from C into Rust - possible
 * rather than merely stated: the caller says what to stop at, gets control
 * back at that point, and decides what happens next.
 *
 * "At that point" is meant exactly. The run stops AT the event, not at the end
 * of the step that crossed it, because a mode switch that happens at the next
 * step boundary happens at a time that depends on the step size, and then two
 * runs of the same plan light the engine at different places (PROJECT.md
 * section 4, "Перемикання режимів — через події, не поперек кроку").
 *
 * The price is paid honestly: stopping at an event does change the step
 * sequence afterwards, because the trajectory resumes from a time the
 * controller did not choose. That is the intended behaviour and not a
 * relaxation of invariant 5 - the same plan replayed stops at the same event
 * time and resumes from the same state, which is what determinism requires.
 * What would break it is the opposite: letting the step size decide where the
 * event happened. */
typedef enum {
    /* Closest approach to a body: the radial rate crosses zero upwards. */
    CORE_EVENT_PERIAPSIS = 0,
    /* Farthest, the same crossing downwards. */
    CORE_EVENT_APOAPSIS = 1,
    /* param metres from the body's centre, crossed in either direction -
     * entering and leaving are both worth stopping for.
     *
     * Distance from the centre, not altitude, and until K7c that was the only
     * one on offer: the asset carried a name and a mu per body and no radius,
     * so an altitude event would have had to invent one. Radii shipped with
     * asset v3 (K6b), and the altitude event below is what they made possible.
     * This one stays, and not merely for compatibility - a sphere of influence
     * or a rendezvous ring is a distance from a centre and has nothing to do
     * with a surface. */
    CORE_EVENT_DISTANCE = 2,

    /* param metres above the body's surface, crossed in either direction
     * (ROADMAP K7c). The atmosphere boundary PROJECT.md section 5 sketches,
     * and the event a re-entry burn is planned against.
     *
     * "Surface" is the asset's mean radius (eph_body_radius), a sphere. Not
     * an ellipsoid and not terrain: the same sphere the atmosphere measures
     * its own altitudes above (core/field.h), so a vessel that stops at
     * 100 km stops where the air says 100 km. Note it is NOT the harmonics'
     * reference radius, which for the Earth is a different number.
     *
     * A body whose asset does not say how big it is (radius 0 - nine of the
     * fixture's ten) is refused with CORE_ERR_INVALID_ARG when the event is
     * armed. Measuring altitude from a radius of zero would silently turn
     * this into CORE_EVENT_DISTANCE, and the caller would be told nothing.
     *
     * THE BANDED ATMOSPHERE DOES NOT REACH THE ROOT FINDER, and this was the
     * one thing worth checking before writing it. The table is piecewise and
     * discontinuous at band boundaries by up to a tenth of a percent (K7a),
     * and a root search across such a seam would be searching for a zero of a
     * function that jumps. But this event's g is |r - R| - radius - param:
     * a distance, which reads no density at all. The seam enters the
     * trajectory as a small jump in acceleration - in g'' - while Newton here
     * uses only g and g'. Measured rather than argued: an event armed exactly
     * on a band base lands as accurately as one armed mid-band
     * (core/test/test_prop.c). */
    CORE_EVENT_ALTITUDE = 3,
} CoreEventKind;

/* Two more are missing, and the reason has changed under them. SHADOW_ENTRY
 * waited on a shadow model and STATION_RISE on body rotation; K6a shipped the
 * first (core/srp.h) and K3b the second (eph_body_orientation), so neither is
 * blocked any more. What neither has is a caller. They arrive with the mission
 * planner that wants them, on the same rule that kept cd out of VesselParams
 * until K7b: a value nobody reads is worse than its absence. */
typedef struct {
    CoreEventKind kind;
    int           body_id;  /* index into the ephemeris */
    /* Metres. Distance from the centre for CORE_EVENT_DISTANCE, height above
     * the surface for CORE_EVENT_ALTITUDE, unused for the apsides. */
    double        param;
} CoreEvent;

#define PROP_MAX_EVENTS 8

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

/* Integrate from *initial to t_end, in the field of every body in the asset,
 * stopping at the first of: t_end, a full buffer, or an armed event.
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
 * Events are evaluated at the end of every accepted step and bracketed
 * between two of them, so an event whose whole excursion fits inside one step
 * is missed - a periapsis fifty metres deep in a step spanning an hour, for
 * instance. That is a property of every event system built this way and not a
 * defect to be fixed by checking more often: the same tolerance that keeps the
 * step honest is what keeps the bracket meaningful. out_event carries the
 * index into events[] of the one that fired, or -1.
 *
 * When an event fires, the sample past it is replaced by the event state, so
 * the polyline in out_states ends exactly where the run ended.
 *
 * vessel says how radiation pressure pushes on this spacecraft and may be
 * NULL; see the head of this file. It is applied for the whole run, so a
 * burn that changes the mass belongs at a leg boundary, which is where the
 * plan already puts it.
 *
 * out_count, out_final, out_stop, out_event and in_out_step are all required;
 * out_states is the only optional one, and events may be NULL with
 * n_events = 0. */
CoreResult prop_run(PropagatorCtx *p, const State *initial,
                    const VesselParams *vessel, double t_end,
                    const CoreEvent *events, size_t n_events,
                    State *out_states, size_t out_cap, size_t *out_count,
                    State *out_final, CoreStopReason *out_stop, int *out_event,
                    double *in_out_step);

/* The same integration, carrying the state transition matrix with it
 * (ROADMAP K8, PROJECT.md section 5).
 *
 * out_stm is row-major 6x6, d y(t_end) / d y(initial), state ordered
 * (x, y, z, vx, vy, vz) - see core/stm.h, which is what does the work here.
 * This is what differential correction asks for in M3 and what pushes a
 * covariance forward in M6.
 *
 * THE TRAJECTORY IS THE SAME ONE. Not approximately, not to within the
 * tolerance: prop_run and prop_run_stm over the same interval, from the
 * same state and the same carried step, produce bit-identical results. The
 * reason is in core/dop853.c and is worth knowing rather than trusting -
 * the step controller's error norm reads block 0 alone, so the six
 * variational blocks ride the step sequence without ever voting on it.
 * core/test/test_prop.c measures this rather than assuming it.
 *
 * That is CLAUDE.md invariant 5 at the point where it is easiest to lose:
 * a planner that corrects a manoeuvre using a matrix belonging to a
 * slightly different trajectory would aim at where the vessel is not.
 *
 * No events and no sample buffer, and neither is an oversight. This
 * answers "where does a change at the start end up at t_end", which is a
 * question about one leg with two ends; an event would end the leg
 * somewhere the caller did not ask about, and the matrix would then
 * describe a different interval than the caller believes. A caller wanting
 * both flies prop_run to find the event, then prop_run_stm over the leg it
 * defines.
 *
 * in_out_step behaves exactly as in prop_run, and for the same reason. */
CoreResult prop_run_stm(PropagatorCtx *p, const State *initial,
                        const VesselParams *vessel, double t_end,
                        State *out_final, double out_stm[36],
                        double *in_out_step);

#endif /* CORE_PROP_H */
