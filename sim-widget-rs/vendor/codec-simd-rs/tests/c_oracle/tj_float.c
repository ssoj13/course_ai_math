/* libjpeg `jpeg_fdct_float` wrapper. The input is a level-shifted integer block converted to float exactly as
 * `convsamp_float` does; the output is written as the raw IEEE bit patterns of the float results so the Rust side
 * compares bits, not values.
 */
#include <stdint.h>
#include <string.h>
#include "jfdctflt.c"

void tj_fdct_float(const int32_t *in, int32_t *out)
{
    FAST_FLOAT d[64];
    for (int i = 0; i < 64; i++) d[i] = (FAST_FLOAT)in[i];
    jpeg_fdct_float(d);
    memcpy(out, d, sizeof d);
}
