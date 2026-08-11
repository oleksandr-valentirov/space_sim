/* Bit-exact hashing of simulation state.
 *
 * Determinism is a hard requirement (PROJECT.md section 4), which means it
 * needs to be measurable. This is the measuring instrument: feed it state,
 * compare the result against a golden value recorded in the repository.
 *
 * FNV-1a, 64 bit. Not cryptographic on purpose — the job is to detect that
 * two builds produced different bits, not to resist an adversary. */

#ifndef CORE_HASH_H
#define CORE_HASH_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint64_t state;
} CoreHash;

void core_hash_init(CoreHash *h);
void core_hash_bytes(CoreHash *h, const void *data, size_t n);

/* Hashes the raw bits of x, so -0.0 and 0.0 hash differently and NaN payloads
 * matter. That is intended: a platform that produces -0.0 where another
 * produces 0.0 has a determinism difference worth catching.
 *
 * Bytes are fed least-significant first regardless of host byte order, so the
 * hash stays comparable if a big-endian target ever appears. */
void core_hash_f64(CoreHash *h, double x);

uint64_t core_hash_value(const CoreHash *h);

#endif /* CORE_HASH_H */
