/* Station-keeping budget (ROADMAP C4).
 *
 * Multiple shooting produces a trajectory that is ballistic on paper. Flying
 * it is a different question: near L2 a perturbation is multiplied by 594 per
 * revolution (core/test/test_stability.c), so a vessel injected a kilometre
 * off, or one whose position is merely known to a kilometre, leaves. What it
 * costs to stay is the number this measures - and the number PROJECT.md
 * section 8 wants the player operating with.
 *
 * The controller is the simplest one that flight dynamics actually uses.
 * Every so often, choose the velocity that will put the vessel on the
 * reference position at some point downstream, and burn the difference. Two
 * numbers set it:
 *
 *   control_interval  how often a manoeuvre may happen
 *   horizon           how far ahead the aim point is
 *
 * The horizon is the interesting one. Aiming at the very next patch point
 * forces the vessel onto the reference immediately and costs a great deal,
 * because it also has to arrive with whatever velocity that demands. Aiming
 * further ahead lets the natural dynamics do most of the work and the burn
 * only cancel the part that would diverge. The cost falls steeply with the
 * horizon and then flattens; core/test/test_station.c measures the curve.
 *
 * A three by three solve, so no libm. */

#ifndef CORE_STATION_H
#define CORE_STATION_H

#include "integrator.h"

typedef struct {
    double tol_m;          /* integrator tolerance for the vessel */

    /* How close the targeter must bring the aim point, in metres. Below the
     * integrator tolerance it only wastes iterations. */
    double target_tol_m;

    int control_interval;  /* patch points between manoeuvres; 0 -> 1 */
    int horizon;           /* patch points ahead to aim at; 0 -> 1 */
    int max_iterations;    /* 0 -> 10 */
} StationConfig;

typedef struct {
    double total_dv;      /* m/s over the whole flight */
    double largest_dv;    /* m/s, the single biggest burn */
    double per_year;      /* total_dv scaled to 365.25 days */

    int    manoeuvres;
    double flown;         /* seconds actually flown */

    /* Largest distance between the vessel and the reference point for the
     * same time, over the whole flight. This is what "stayed on station"
     * means, and it is not what the targeter drives to zero - the targeter
     * only constrains the aim points. */
    double worst_offset_m;

    int    completed;     /* 1 if the whole reference was flown */
} StationReport;

/* Fly from `initial` along a reference trajectory, correcting as configured.
 *
 * reference[] and times[] are what shoot_multiple produced: n states that are
 * continuous under the same dynamics f. initial is where the vessel actually
 * is at times[0] - pass reference[0] itself for a perfectly injected vessel,
 * which should cost almost nothing, or a displaced copy to measure the price
 * of an error.
 *
 * Stops early with completed = 0 if a leg fails to converge, reporting what
 * was flown up to that point. */
CoreResult station_keep(BlockAccelFunc f, void *ctx,
                        const State *reference, const double *times, size_t n,
                        const State *initial,
                        const StationConfig *cfg, StationReport *out);

#endif /* CORE_STATION_H */
