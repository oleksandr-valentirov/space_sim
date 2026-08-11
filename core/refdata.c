#include "refdata.h"

#include <stdio.h>
#include <string.h>

#define KM_TO_M   1000.0
#define KM_S_TO_M_S 1000.0

#define LINE_MAX 512

static int is_blank_or_comment(const char *line)
{
    while (*line == ' ' || *line == '\t') {
        line++;
    }
    return *line == '#' || *line == '\n' || *line == '\r' || *line == '\0';
}

CoreResult refdata_load_vectors(const char *path,
                                RefSample *out, size_t cap, size_t *out_count)
{
    if (path == NULL || out == NULL || out_count == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    FILE *f = fopen(path, "r");
    if (f == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    char line[LINE_MAX];
    size_t n = 0;
    CoreResult result = CORE_OK;

    while (fgets(line, sizeof line, f) != NULL) {
        if (is_blank_or_comment(line)) {
            continue;
        }

        double jd, x, y, z, vx, vy, vz;
        if (sscanf(line, "%lf,%lf,%lf,%lf,%lf,%lf,%lf",
                   &jd, &x, &y, &z, &vx, &vy, &vz) != 7) {
            result = CORE_ERR_INVALID_ARG;
            break;
        }

        if (n >= cap) {
            result = CORE_ERR_BUFFER_TOO_SMALL;
            break;
        }

        out[n].jdtdb = jd;
        out[n].s.r = vec3(x * KM_TO_M, y * KM_TO_M, z * KM_TO_M);
        out[n].s.v = vec3(vx * KM_S_TO_M_S, vy * KM_S_TO_M_S, vz * KM_S_TO_M_S);
        out[n].s.t = (jd - REFDATA_JD_J2000) * REFDATA_SEC_PER_DAY;
        n++;
    }

    fclose(f);
    *out_count = n;
    return result;
}

CoreResult refdata_load_gm(const char *path,
                           RefGm *out, size_t cap, size_t *out_count)
{
    if (path == NULL || out == NULL || out_count == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    FILE *f = fopen(path, "r");
    if (f == NULL) {
        return CORE_ERR_INVALID_ARG;
    }

    char line[LINE_MAX];
    size_t n = 0;
    CoreResult result = CORE_OK;

    while (fgets(line, sizeof line, f) != NULL) {
        if (is_blank_or_comment(line)) {
            continue;
        }

        char name[32];
        double gm_km;
        if (sscanf(line, "%31[^,],%lf", name, &gm_km) != 2) {
            result = CORE_ERR_INVALID_ARG;
            break;
        }

        if (n >= cap) {
            result = CORE_ERR_BUFFER_TOO_SMALL;
            break;
        }

        /* snprintf rather than memcpy: sscanf leaves the tail of name
         * uninitialised, and copying the whole buffer would read it. */
        snprintf(out[n].name, sizeof out[n].name, "%s", name);

        /* km^3/s^2 to m^3/s^2. */
        out[n].gm = gm_km * (KM_TO_M * KM_TO_M * KM_TO_M);
        n++;
    }

    fclose(f);
    *out_count = n;
    return result;
}

double refdata_gm_of(const RefGm *table, size_t n, const char *name)
{
    for (size_t i = 0; i < n; i++) {
        if (strcmp(table[i].name, name) == 0) {
            return table[i].gm;
        }
    }
    return 0.0;
}
