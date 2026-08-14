/* Reading GRAIL (ROADMAP K5e). Run from the repository root.
 *
 * The oracle is the file's own label plus two facts about the Moon that are
 * known independently of any particular model: it is slightly oblate, and it
 * is far more triaxial than the Earth - C_22 is a fifth of C_20 rather than a
 * millionth. A parser that dropped a column, mixed C with S, or shifted the
 * triangular index would break one of those. */

#include "harmonics.h"
#include "sha_read.h"
#include "test.h"

#include <math.h>
#include <stdio.h>

#define PATH "data/grail/grgm900c_d50_sha.tab"

int main(void)
{
    HarmonicsField f;
    ShaReport rep;

    CoreResult res = sha_read(PATH, HARMONICS_MAX_DEGREE, &f, &rep);
    if (res != CORE_OK) {
        fprintf(stderr, "  cannot read %s; run from the repository root\n",
                PATH);
        return EXIT_FAILURE;
    }

    /* Straight from gggrx_0900c_sha.lbl: reference radius 1738.0 km, GM
     * 4902.79996708864 km^3/s^2, degree and order 900. */
    CHECK(f.re == 1738000.0);
    CHECK(rep.file_degree == 900);
    CHECK(fabs(rep.mu - 4.90279996708864e12) < 1.0);

    /* The file was truncated to exactly the degree the core can carry. */
    CHECK(rep.read_degree == HARMONICS_MAX_DEGREE);
    CHECK(f.degree == HARMONICS_MAX_DEGREE);

    /* Every pair of degree 2..50 and no others: 1325 lines in the file, of
     * which 3 are degrees 0 and 1. */
    long expect = 0;
    for (int n = 2; n <= HARMONICS_MAX_DEGREE; n++) {
        expect += n + 1;
    }
    CHECK(rep.pairs_read == expect);

    double c20 = f.c[harmonics_index(2, 0)];
    double c22 = f.c[harmonics_index(2, 2)];
    double s22 = f.s[harmonics_index(2, 2)];

    /* Oblate, so C_20 is negative; and this is the one number in the file
     * that can be checked against a value known from elsewhere entirely -
     * the Moon's J2 is about 2.03e-4 unnormalised, which is 9.09e-5 once
     * divided by N_20 = sqrt(5). Agreement to a percent here means the
     * normalisation convention was read right, not just the digits. */
    CHECK(c20 < 0.0);
    double j2 = -c20 * harmonics_normalisation(2, 0);
    printf("  lunar J2 from the file: %.4e (known ~2.03e-4)\n", j2);
    CHECK(j2 > 2.0e-4 && j2 < 2.06e-4);

    /* Triaxiality: the Moon's C_22 is famously large - about a fifth of
     * C_20, where the Earth's is a millionth of its own. A parser reading
     * the S column into C, or shifting a row, would not produce that. */
    CHECK(c22 > 0.0);
    double ratio = c22 / fabs(c20);
    printf("  C22/|C20| = %.3f (the Moon is triaxial; Earth's is ~1e-6)\n",
           ratio);
    CHECK(ratio > 0.1 && ratio < 0.5);

    /* S_22 is small but not zero: the principal axes are defined so that it
     * nearly vanishes, and "nearly" is the difference between a real model
     * and a tidied one. */
    CHECK(s22 != 0.0);
    CHECK(fabs(s22) < 0.01 * c22);

    /* The high end is populated too, or the truncation clipped the wrong
     * place: a Kaula-like field has terms of order 1e-7 at degree 50. */
    double top = fabs(f.c[harmonics_index(50, 50)]);
    CHECK(top > 1.0e-9 && top < 1.0e-5);

    /* Reading it twice gives the same thing, and asking for less gives less
     * without changing what it does give. */
    HarmonicsField small;
    ShaReport rep_small;
    CHECK(sha_read(PATH, 8, &small, &rep_small) == CORE_OK);
    CHECK(small.degree == 8);
    CHECK(rep_small.pairs_read < rep.pairs_read);
    CHECK_BITS_EQ(small.c[harmonics_index(2, 0)], c20);
    CHECK_BITS_EQ(small.c[harmonics_index(8, 3)], f.c[harmonics_index(8, 3)]);
    CHECK(small.c[harmonics_index(9, 0)] == 0.0);

    /* Refusals, each for a reason the header states. */
    HarmonicsField ignored;
    ShaReport ignored_rep;
    CHECK(sha_read("data/grail/does-not-exist.tab", 8, &ignored, &ignored_rep)
          == CORE_ERR_INVALID_ARG);
    CHECK(sha_read(PATH, 1, &ignored, &ignored_rep) == CORE_ERR_INVALID_ARG);
    CHECK(sha_read(NULL, 8, &ignored, &ignored_rep) == CORE_ERR_INVALID_ARG);

    /* And a file that is not this format at all: the GM table, which is a
     * CSV with a text header. It must be refused rather than parsed into
     * whatever the digits happen to spell. */
    CHECK(sha_read("data/horizons/gm.csv", 8, &ignored, &ignored_rep)
          == CORE_ERR_INVALID_ARG);

    printf("  %ld pairs to degree %d, reference radius %.1f km\n",
           rep.pairs_read, rep.read_degree, rep.reference_radius_m / 1000.0);

    return TEST_RESULT();
}
