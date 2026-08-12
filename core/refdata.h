/* Loading JPL Horizons reference data (ROADMAP B1).
 *
 * This is offline, cooker-side code: it runs when the ephemeris asset is
 * built and when tests measure the integrator against JPL, never inside the
 * propagation loop. That distinction matters, because decimal to double
 * conversion is not guaranteed identical across C libraries, and the runtime
 * must never depend on it. The runtime reads a binary asset (PROJECT.md
 * section 4); text parsing stays here.
 *
 * Units are converted on load: Horizons publishes km and km/s, everything
 * inside the core is metres and m/s (vec3.h). */

#ifndef CORE_REFDATA_H
#define CORE_REFDATA_H

#include "core.h"

#include <stddef.h>

/* Julian date of the J2000.0 epoch in TDB. State.t counts seconds from here. */
#define REFDATA_JD_J2000 2451545.0
#define REFDATA_SEC_PER_DAY 86400.0

typedef struct {
    double jdtdb;
    State  s;      /* metres, m/s, t in seconds from J2000 TDB */
} RefSample;

typedef struct {
    char   name[32];
    double gm;     /* m^3/s^2 */
} RefGm;

/* Both loaders follow the boundary rule from PROJECT.md section 5 even though
 * nothing crosses the boundary yet: the caller supplies the buffer and its
 * capacity, and gets back how much was written. Allocating here would mean
 * inventing an ownership question that does not need to exist. */

CoreResult refdata_load_vectors(const char *path,
                                RefSample *out, size_t cap, size_t *out_count);

CoreResult refdata_load_gm(const char *path,
                           RefGm *out, size_t cap, size_t *out_count);

/* Returns 0.0 when the name is absent. Callers that cannot tolerate a missing
 * body must check for it; silently propagating a zero GM would produce a
 * plausible-looking trajectory with no gravity in it. */
double refdata_gm_of(const RefGm *table, size_t n, const char *name);

/* A published periodic orbit from the JPL three-body catalogue (ROADMAP C2).
 * Dimensionless CR3BP units; see data/jpl_halo/. */
typedef struct {
    int    index;      /* position in the catalogue, for traceability */
    State  s;          /* initial state, t = 0 */
    double jacobi;
    double period;
    double stability;  /* max eigenvalue magnitude of the monodromy matrix */
} RefHalo;

CoreResult refdata_load_halo(const char *path,
                             RefHalo *out, size_t cap, size_t *out_count);

/* A file holding one number and any number of comment lines. Exists because
 * the halo catalogue's mass ratio must travel with the orbits rather than be
 * recomputed from GM values that disagree with it in the eighth digit. */
CoreResult refdata_load_scalar(const char *path, double *out);

#endif /* CORE_REFDATA_H */
