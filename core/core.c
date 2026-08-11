#include "core.h"

const char *core_result_str(CoreResult r)
{
    switch (r) {
    case CORE_OK:                       return "CORE_OK";
    case CORE_ERR_BUFFER_TOO_SMALL:     return "CORE_ERR_BUFFER_TOO_SMALL";
    case CORE_ERR_TOLERANCE_NOT_MET:    return "CORE_ERR_TOLERANCE_NOT_MET";
    case CORE_ERR_INVALID_ARG:          return "CORE_ERR_INVALID_ARG";
    }
    /* Reached only if a caller passes a value outside the enum. */
    return "CORE_ERR_UNKNOWN";
}
