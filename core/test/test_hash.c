#include "hash.h"
#include "test.h"

static uint64_t hash_str(const char *s)
{
    CoreHash h;
    core_hash_init(&h);
    core_hash_bytes(&h, s, strlen(s));
    return core_hash_value(&h);
}

static uint64_t hash_f64(double x)
{
    CoreHash h;
    core_hash_init(&h);
    core_hash_f64(&h, x);
    return core_hash_value(&h);
}

int main(void)
{
    /* Published FNV-1a 64 test vectors. If these ever fail, the hash itself
     * drifted, and every golden value in the repository is meaningless. */
    CHECK(hash_str("") == 0xcbf29ce484222325ULL);
    CHECK(hash_str("a") == 0xaf63dc4c8601ec8cULL);
    CHECK(hash_str("foobar") == 0x85944171f73967e8ULL);

    /* Distinctions the hash must preserve, because they are real determinism
     * differences rather than noise. */
    CHECK(hash_f64(0.0) != hash_f64(-0.0));

    /* One ULP apart, built by bit arithmetic rather than by adding a small
     * constant: at 1.0 the gap is about 2.2e-16, so anything smaller would
     * round back to 1.0 and quietly test nothing. */
    double one = 1.0, one_ulp;
    uint64_t bits;
    memcpy(&bits, &one, sizeof bits);
    bits += 1;
    memcpy(&one_ulp, &bits, sizeof one_ulp);
    CHECK(one != one_ulp);
    CHECK(hash_f64(one) != hash_f64(one_ulp));

    /* ... and one it must not invent. */
    CHECK(hash_f64(0.1 + 0.2) == hash_f64(0.30000000000000004));

    /* Order matters: hashing the same values in a different sequence must not
     * collide, or a reordered integration would pass unnoticed. */
    CoreHash a, b;
    core_hash_init(&a);
    core_hash_f64(&a, 1.0);
    core_hash_f64(&a, 2.0);
    core_hash_init(&b);
    core_hash_f64(&b, 2.0);
    core_hash_f64(&b, 1.0);
    CHECK(core_hash_value(&a) != core_hash_value(&b));

    return TEST_RESULT();
}
