#include "hash.h"

#include <string.h>

#define FNV_OFFSET_BASIS 0xcbf29ce484222325ULL
#define FNV_PRIME        0x00000100000001b3ULL

void core_hash_init(CoreHash *h)
{
    h->state = FNV_OFFSET_BASIS;
}

void core_hash_bytes(CoreHash *h, const void *data, size_t n)
{
    const unsigned char *p = (const unsigned char *)data;
    for (size_t i = 0; i < n; i++) {
        h->state ^= (uint64_t)p[i];
        h->state *= FNV_PRIME;
    }
}

void core_hash_f64(CoreHash *h, double x)
{
    uint64_t bits;
    memcpy(&bits, &x, sizeof bits);

    for (unsigned i = 0; i < 8; i++) {
        unsigned char byte = (unsigned char)((bits >> (8 * i)) & 0xffu);
        core_hash_bytes(h, &byte, 1);
    }
}

uint64_t core_hash_value(const CoreHash *h)
{
    return h->state;
}
