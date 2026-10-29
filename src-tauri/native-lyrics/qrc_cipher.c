#include <stddef.h>
#include "des.h"

/* QRC cache cipher order based on QQMusicMonitor (MIT). The underlying DES
 * implementation is by Brad Conte. Inputs are limited and block-aligned by
 * the caller and checked again here before entering the cipher. */
int cf_qrc_transform(unsigned char *bytes, size_t length, int decode) {
    static const BYTE keys[3][8] = { "!@#)(NHL", "123ZXC!@", "!@#)(*$%" };
    BYTE schedule[16][6];
    if (!bytes || length == 0 || length > 524288 || length % 8 != 0) return 0;
    for (int step = 0; step < 3; ++step) {
        int key = decode ? step : 2 - step;
        DES_MODE mode = decode ? (step == 1 ? DES_ENCRYPT : DES_DECRYPT)
                               : (step == 1 ? DES_DECRYPT : DES_ENCRYPT);
        des_key_setup(keys[key], schedule, mode);
        for (size_t offset = 0; offset < length; offset += 8)
            des_crypt(bytes + offset, bytes + offset, schedule);
    }
    return 1;
}
