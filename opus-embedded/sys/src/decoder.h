#include <opus.h>

/**
 * Struct big enough to contain OpusDecoder of opus-1.5.2 so it can be reserved
 * on stack. Alignment is taken care of by libopus.
 * <div rustbindgen replaces="OpusDecoder"></div>
 */
struct OpusDecoder {
#ifdef OPUS_EMBEDDED_SYS_STEREO
    char _unused[26580] __attribute__((aligned(4)));
#else
    char _unused[17860] __attribute__((aligned(4)));
#endif
};
