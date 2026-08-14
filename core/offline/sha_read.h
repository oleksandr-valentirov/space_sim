/* Reading a published spherical-harmonic model - OFFLINE ONLY (ROADMAP K5e).
 *
 * The PDS "SHADR" ASCII form: one header line, then one line per coefficient
 * pair, `n, m, C, S, sigma_C, sigma_S`. data/grail/README.md describes the
 * file this was written for and where it came from.
 *
 * Offline, and that is the whole reason this file exists rather than the
 * reader living in core/: the cooker turns a text table into asset bytes
 * once, on a developer's machine, and the runtime reads only the asset. So
 * this may use stdio and strtod, and a mis-parsed digit becomes a failed cook
 * rather than a trajectory nobody can explain.
 *
 * COEFFICIENTS ARE COPIED, NOT CONVERTED. The published model is fully
 * normalised and so is HarmonicsField since K5b, which is not a coincidence:
 * both are normalised for the same reason, that the unnormalised form
 * overflows a double around degree 44. A reader that "helpfully" normalised
 * again would be the kind of silent factor this project keeps out of the
 * boundary. The file states its own convention in the header, and this
 * refuses anything that does not say 1. */

#ifndef CORE_SHA_READ_H
#define CORE_SHA_READ_H

#include "core.h"
#include "harmonics.h"

typedef struct {
    /* Straight from the file's header line, for the caller to check against
     * what it believes it asked for. */
    double reference_radius_m;
    double mu;                  /* m^3/s^2, converted from the file's km^3/s^2 */
    int    file_degree;         /* the model's own degree, before truncation */
    int    read_degree;         /* the highest degree actually loaded */
    long   pairs_read;
} ShaReport;

/* Fill `out` with every term of degree 2..max_degree the file carries.
 *
 * Degrees 0 and 1 are skipped rather than stored: degree 0 is the point mass
 * the ephemeris already carries as mu, and degree 1 vanishes in a frame
 * centred on the centre of mass. GRAIL's file writes them as exact zeros, and
 * this checks that rather than assuming it - a non-zero degree 1 would mean
 * the model is referenced to something other than the centre of mass, which
 * would move every trajectory near the body.
 *
 * max_degree is clamped to HARMONICS_MAX_DEGREE. Terms beyond it are ignored,
 * which is what truncating a gravity field means; the report says what was
 * actually read so a caller can print it rather than assume.
 *
 * Errors: CORE_ERR_INVALID_ARG for a missing file, a header this code does
 * not recognise, a model that says it is unnormalised, a non-zero degree-1
 * term, or a line that does not parse. Nothing is reported by returning a
 * half-filled field. */
CoreResult sha_read(const char *path, int max_degree, HarmonicsField *out,
                    ShaReport *report);

#endif /* CORE_SHA_READ_H */
