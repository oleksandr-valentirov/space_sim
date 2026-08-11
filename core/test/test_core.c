#include "core.h"
#include "test.h"

int main(void)
{
    CHECK(CORE_OK == 0);

    CHECK(strcmp(core_result_str(CORE_OK), "CORE_OK") == 0);
    CHECK(strcmp(core_result_str(CORE_ERR_INVALID_ARG),
                 "CORE_ERR_INVALID_ARG") == 0);

    /* Out-of-range values must still return a usable string, not NULL:
     * the Rust wrapper will call this on whatever the FFI boundary hands it. */
    CHECK(core_result_str((CoreResult)999) != NULL);

    /* The harness itself: bit-exact comparison is what it claims to be. */
    CHECK_BITS_EQ(0.1 + 0.2, 0.30000000000000004);

    return TEST_RESULT();
}
