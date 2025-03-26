// Custom allocation
// Nothing special, free to use for any purpose

#include <stddef.h>

#define OVERRIDE_OPUS_ALLOC
static inline void *opus_alloc (size_t size)
{
    (void)size;
    return NULL;
}

#define OVERRIDE_OPUS_REALLOC
static inline void *opus_realloc (void *ptr, size_t size)
{
    (void)ptr;
    (void)size;
    return NULL;
}

#define OVERRIDE_OPUS_FREE
static inline void opus_free (void *ptr)
{
    (void)ptr;
}
