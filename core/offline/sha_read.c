#include "sha_read.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The header line carries eight comma-separated fields; only the first six
 * mean anything here. Parsed with strtod rather than scanf so that a field
 * that is not a number is caught where it sits instead of stopping the whole
 * line silently. */
static int parse_header(const char *line, double *radius_km, double *mu_km3,
                        int *degree, int *normalised)
{
    const char *p = line;
    char *end = NULL;

    double values[6];
    for (int i = 0; i < 6; i++) {
        values[i] = strtod(p, &end);
        if (end == p) {
            return 0;
        }
        p = end;
        while (*p == ' ' || *p == '\t') {
            p++;
        }
        if (i < 5) {
            if (*p != ',') {
                return 0;
            }
            p++;
        }
    }

    *radius_km = values[0];
    *mu_km3 = values[1];
    *degree = (int)values[3];
    *normalised = (int)values[5];
    return 1;
}

CoreResult sha_read(const char *path, int max_degree, HarmonicsField *out,
                    ShaReport *report)
{
    if (path == NULL || out == NULL || report == NULL || max_degree < 2) {
        return CORE_ERR_INVALID_ARG;
    }

    if (max_degree > HARMONICS_MAX_DEGREE) {
        max_degree = HARMONICS_MAX_DEGREE;
    }

    FILE *f = fopen(path, "r");
    if (f == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    memset(out, 0, sizeof *out);
    memset(report, 0, sizeof *report);

    char line[256];
    if (fgets(line, sizeof line, f) == NULL) {
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    double radius_km = 0.0, mu_km3 = 0.0;
    int file_degree = 0, normalised = 0;
    if (!parse_header(line, &radius_km, &mu_km3, &file_degree, &normalised)) {
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    /* A model in the other convention is refused rather than converted. The
     * conversion exists (harmonics_normalisation), but a file that says 0
     * here is not the file this was written for, and guessing at the rest of
     * its layout would be worse than stopping. */
    if (normalised != 1 || !(radius_km > 0.0) || !(mu_km3 > 0.0)) {
        fclose(f);
        return CORE_ERR_INVALID_ARG;
    }

    out->re = radius_km * 1000.0;
    out->degree = 0;

    long pairs = 0;
    int highest = 0;

    while (fgets(line, sizeof line, f) != NULL) {
        const char *p = line;
        char *end = NULL;

        long n = strtol(p, &end, 10);
        if (end == p) {
            continue;   /* blank or padding line */
        }
        p = end;
        while (*p == ' ' || *p == ',') {
            p++;
        }

        long m = strtol(p, &end, 10);
        if (end == p) {
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        p = end;
        while (*p == ' ' || *p == ',') {
            p++;
        }

        double c = strtod(p, &end);
        if (end == p) {
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }
        p = end;
        while (*p == ' ' || *p == ',') {
            p++;
        }

        double s = strtod(p, &end);
        if (end == p) {
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        if (n < 0 || m < 0 || m > n) {
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        /* Degree 1 must be zero, or the model is referenced to something
         * other than the centre of mass and every term below is measured
         * from the wrong origin. Checked rather than trusted. */
        if (n == 1 && (c != 0.0 || s != 0.0)) {
            fclose(f);
            return CORE_ERR_INVALID_ARG;
        }

        if (n < 2) {
            continue;
        }
        if (n > max_degree) {
            break;   /* the file is sorted by degree */
        }

        out->c[harmonics_index((int)n, (int)m)] = c;
        out->s[harmonics_index((int)n, (int)m)] = s;
        pairs++;
        if ((int)n > highest) {
            highest = (int)n;
        }
    }

    fclose(f);

    if (highest < 2) {
        return CORE_ERR_INVALID_ARG;
    }

    out->degree = highest;

    report->reference_radius_m = out->re;
    report->mu = mu_km3 * 1.0e9;   /* km^3/s^2 -> m^3/s^2 */
    report->file_degree = file_degree;
    report->read_degree = highest;
    report->pairs_read = pairs;

    return CORE_OK;
}
