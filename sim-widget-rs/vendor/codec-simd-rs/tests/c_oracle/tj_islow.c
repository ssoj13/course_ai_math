/* libjpeg-turbo `jpeg_fdct_islow` wrapper: compiled twice by gen_fdct_kat.ps1.
 *   -DBITS_IN_JSAMPLE=8  -DKAT_SIMD  -> DCTELEM = short (the layout of a WITH_SIMD build), PASS1_BITS 2, MULTIPLY16C16
 *   -DBITS_IN_JSAMPLE=8  -DKAT_INT   -> DCTELEM = int (the portable build), PASS1_BITS 2 (symbol renamed on the command line)
 *   -DBITS_IN_JSAMPLE=12             -> DCTELEM = JLONG, PASS1_BITS 1, full multiply
 */
#include <stdint.h>
#include "jfdctint.c"

#if BITS_IN_JSAMPLE == 8 && defined(KAT_INT)
#define KAT_NAME tj_fdct_islow_8i
#define KAT_FN   jpeg_fdct_islow
#elif BITS_IN_JSAMPLE == 8
#define KAT_NAME tj_fdct_islow_8
#define KAT_FN   jpeg_fdct_islow
#else
#define KAT_NAME tj_fdct_islow_12
#define KAT_FN   jpeg12_fdct_islow
#endif

void KAT_NAME(const int32_t *in, int32_t *out)
{
    DCTELEM d[64];
    for (int i = 0; i < 64; i++) d[i] = (DCTELEM)in[i];
    KAT_FN(d);
    for (int i = 0; i < 64; i++) out[i] = (int32_t)d[i];
}
