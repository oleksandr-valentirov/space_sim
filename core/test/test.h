/* Minimal unit test harness for the core.
 *
 * No dependencies on purpose: the core has none, and its tests should not
 * introduce any. One executable per test file; the Makefile discovers them by
 * globbing the C files under core/test.
 *
 *   #include "test.h"
 *   int main(void) {
 *       CHECK(2 + 2 == 4);
 *       return TEST_RESULT();
 *   }
 */

#ifndef CORE_TEST_H
#define CORE_TEST_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int core_test_failures = 0;

#define CHECK(cond)                                                     \
    do {                                                                \
        if (!(cond)) {                                                  \
            fprintf(stderr, "  FAIL %s:%d: %s\n",                       \
                    __FILE__, __LINE__, #cond);                         \
            core_test_failures++;                                       \
        }                                                               \
    } while (0)

/* Bit-exact comparison of doubles. Deliberately not an epsilon compare:
 * where the core promises determinism (PROJECT.md section 4), "close enough"
 * is not the property under test. Use CHECK with an explicit tolerance where
 * an approximation is what you actually mean. */
#define CHECK_BITS_EQ(a, b)                                             \
    do {                                                                \
        double core_test_a_ = (a);                                      \
        double core_test_b_ = (b);                                      \
        if (memcmp(&core_test_a_, &core_test_b_, sizeof(double)) != 0) { \
            fprintf(stderr, "  FAIL %s:%d: %s != %s (%.17g vs %.17g)\n", \
                    __FILE__, __LINE__, #a, #b,                         \
                    core_test_a_, core_test_b_);                        \
            core_test_failures++;                                       \
        }                                                               \
    } while (0)

#define TEST_RESULT()                                                   \
    (core_test_failures == 0                                            \
        ? (printf("  ok\n"), EXIT_SUCCESS)                              \
        : (fprintf(stderr, "  %d failure(s)\n", core_test_failures),    \
           EXIT_FAILURE))

#endif /* CORE_TEST_H */
