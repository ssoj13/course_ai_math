/* libjpeg-turbo `jpeg_fdct_ifast` wrapper (portable C build: DCTELEM = int at 8 bit, JLONG at 12 bit): compiled twice
 * by gen_fdct_kat.ps1, -DBITS_IN_JSAMPLE=8 and -DBITS_IN_JSAMPLE=12 (the symbols are renamed with -D on the command line).
 */
#include <stdint.h>
#include "jfdctfst.c"

#if BITS_IN_JSAMPLE == 8
#define KAT_NAME tj_fdct_ifast_8
#define KAT_FN   jpeg_fdct_ifast
#else
#define KAT_NAME tj_fdct_ifast_12
#define KAT_FN   jpeg12_fdct_ifast
#endif

void KAT_NAME(const int32_t *in, int32_t *out)
{
    DCTELEM d[64];
    for (int i = 0; i < 64; i++) d[i] = (DCTELEM)in[i];
    KAT_FN(d);
    for (int i = 0; i < 64; i++) out[i] = (int32_t)d[i];
}
