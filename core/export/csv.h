/* Writing CSV, for the Milestone 0 delivery.
 *
 * The core has been measured from the start, but only ever through assertions
 * and single numbers printed by a test. This is the other half: the same
 * quantities written out in full so they can be plotted and looked at
 * (scripts/plot.py). A wrong trajectory that satisfies every tolerance is a
 * thing that happens, and a picture is how it gets caught.
 *
 * Nothing here is part of the runtime. These files are diagnostics, never
 * inputs - which is exactly why formatting doubles as decimal text is allowed
 * here and forbidden in core/ephemeris.c: no CSV is ever compared bit for bit
 * or read back into the simulation, so the fact that printf's last digit is a
 * libc's business and not IEEE's costs nothing. The asset stays binary.
 *
 * %.17g all the same, because a value that has to be squinted at is worth
 * having exactly. Seventeen significant digits round-trip every double. */

#ifndef CORE_EXPORT_CSV_H
#define CORE_EXPORT_CSV_H

#include <stdio.h>

typedef struct {
    FILE       *f;
    const char *path;
    long        rows;
    int         columns;   /* from the header, so a short row is caught */
} Csv;

/* Creates path and writes the header line. Returns 0 and complains on stderr
 * if the file cannot be opened; the caller should give up rather than carry on
 * writing to a NULL. */
int csv_open(Csv *c, const char *path, const char *header);

/* One row of n values. n must match the header's column count. */
void csv_row(Csv *c, int n, ...);

/* One row whose first field is a name and the rest are numbers. */
void csv_named(Csv *c, const char *name, int n, ...);

/* Closes and reports on stdout. Returns 0 if anything went wrong along the
 * way - a full disk shows up here and nowhere earlier. */
int csv_close(Csv *c);

#endif /* CORE_EXPORT_CSV_H */
