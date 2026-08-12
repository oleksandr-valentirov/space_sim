#include "csv.h"

#include <stdarg.h>
#include <string.h>

static int count_columns(const char *header)
{
    int n = 1;
    for (const char *p = header; *p != '\0'; p++) {
        if (*p == ',') {
            n++;
        }
    }
    return n;
}

int csv_open(Csv *c, const char *path, const char *header)
{
    memset(c, 0, sizeof *c);

    c->f = fopen(path, "w");
    if (c->f == NULL) {
        fprintf(stderr, "csv: cannot write %s\n", path);
        fprintf(stderr, "  run from the repository root; `make csv` creates "
                        "the directory\n");
        return 0;
    }

    c->path = path;
    c->columns = count_columns(header);
    fprintf(c->f, "%s\n", header);
    return 1;
}

/* The column check is not ceremony. These files are written by one program and
 * read by another, and a row one field short does not fail - it shifts every
 * column after it and produces a plot that is wrong in a way nobody questions,
 * because it still looks like a trajectory. */
static void check_columns(Csv *c, int n)
{
    if (n != c->columns) {
        fprintf(stderr, "csv: %s has %d columns, row has %d\n",
                c->path, c->columns, n);
        c->columns = -1;   /* remembered, so csv_close fails */
    }
}

static void write_values(Csv *c, int n, int first, va_list ap)
{
    for (int i = 0; i < n; i++) {
        fprintf(c->f, "%s%.17g", (i == 0 && first) ? "" : ",",
                va_arg(ap, double));
    }
    fputc('\n', c->f);
    c->rows++;
}

void csv_row(Csv *c, int n, ...)
{
    if (c->f == NULL) {
        return;
    }
    check_columns(c, n);

    va_list ap;
    va_start(ap, n);
    write_values(c, n, 1, ap);
    va_end(ap);
}

void csv_named(Csv *c, const char *name, int n, ...)
{
    if (c->f == NULL) {
        return;
    }
    check_columns(c, n + 1);

    fputs(name, c->f);

    va_list ap;
    va_start(ap, n);
    write_values(c, n, 0, ap);
    va_end(ap);
}

int csv_close(Csv *c)
{
    if (c->f == NULL) {
        return 0;
    }

    int bad = ferror(c->f) || c->columns < 0;
    if (fclose(c->f) != 0) {
        bad = 1;
    }
    c->f = NULL;

    if (bad) {
        fprintf(stderr, "csv: %s is not complete\n", c->path);
        return 0;
    }

    printf("  %-32s %ld rows\n", c->path, c->rows);
    return 1;
}
