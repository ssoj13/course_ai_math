/* C oracle of the codec-simd FDCT kernels (TEST-ONLY tooling; the shipping crate is pure Rust).
 *
 * Compiles the REFERENCE sources unmodified:
 *   FFmpeg 9.0.1 libavcodec/jfdctint_template.c  (ff_jpeg_fdct_islow_8 / _10), jfdctfst.c (ff_fdct_ifast)
 *   libjpeg-turbo  src/jfdctint.c                (jpeg_fdct_islow, 8-bit DCTELEM=short and 12-bit DCTELEM=int)
 * and prints one line per (kernel, block): `kernel in[64] -> out[64]`. gen_fdct_kat.ps1 builds and runs it; the
 * output is committed as tests/data/fdct_kat.txt and replayed by tests/fdct_kat.rs.
 */
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* ---- FFmpeg ---------------------------------------------------------------------------------------- */
#define BIT_DEPTH 8
#include "jfdctint_template.c"
#undef BIT_DEPTH
#define BIT_DEPTH 10
#include "jfdctint_template.c"
#undef BIT_DEPTH
#include "jfdctfst.c"
#define row_fdct faan_row_fdct /* jfdctfst.c has its own static row_fdct */
#include "faandct.c"
#undef row_fdct

/* ---- libjpeg-turbo (8-bit unit, DCTELEM = short as in a WITH_SIMD build) -------------------------------- */
void tj_fdct_islow_8(const int32_t *in, int32_t *out);   /* tj_islow_8.c  */
void tj_fdct_islow_12(const int32_t *in, int32_t *out);  /* tj_islow_12.c */
void tj_fdct_islow_8i(const int32_t *in, int32_t *out);  /* tj_islow.c, DCTELEM = int (portable build) */
void tj_fdct_ifast_8(const int32_t *in, int32_t *out);   /* tj_ifast.c */
void tj_fdct_ifast_12(const int32_t *in, int32_t *out);
void tj_fdct_float(const int32_t *in, int32_t *out);     /* tj_float.c, output = float bit patterns */

static uint32_t rng = 0x1234567u;
static uint32_t next(void) { rng = rng * 1664525u + 1013904223u; return rng >> 8; }

static void print_block(const char *name, const int32_t *in, const int32_t *out)
{
    printf("%s", name);
    for (int i = 0; i < 64; i++) printf(" %d", in[i]);
    printf(" ->");
    for (int i = 0; i < 64; i++) printf(" %d", out[i]);
    printf("\n");
}

/* One block per call: `lo..hi` is the sample range the kernel is specified for. */
static void gen(int32_t *b, int lo, int hi, int mode)
{
    for (int i = 0; i < 64; i++) {
        switch (mode) {
        case 0: b[i] = lo + (int)(next() % (uint32_t)(hi - lo + 1)); break;        /* noise */
        case 1: b[i] = hi; break;                                                /* all max */
        case 2: b[i] = lo; break;                                                /* all min */
        case 3: b[i] = ((i / 8 + i) & 1) ? hi : lo; break;                       /* checkerboard */
        case 4: b[i] = (i % 8) < 4 ? lo : hi; break;                             /* vertical edge */
        case 5: b[i] = (i / 8) < 4 ? hi : lo; break;                             /* horizontal edge */
        case 6: b[i] = (i == 0) ? hi : 0; break;                                 /* impulse */
        default: b[i] = (i * 7) % (hi - lo + 1) + lo; break;                     /* ramp */
        }
    }
}

int main(void)
{
    struct { const char *name; int lo, hi; } k[] = {
        { "ff_islow_8", -128, 127 }, { "ff_islow_10", -512, 511 }, { "ff_ifast", -128, 127 },
        { "tj_islow_8", -128, 127 }, { "tj_islow_12", -2048, 2047 }, { "ff_faan", -128, 127 }, { "ff_faan_u8", 0, 255 },
        { "tj_islow_8i", -128, 127 }, { "tj_ifast_8", -128, 127 }, { "tj_ifast_12", -2048, 2047 }, { "tj_float", -128, 127 },
    };
    for (int kk = 0; kk < 11; kk++) {
        for (int n = 0; n < 40; n++) {
            int32_t in[64], out[64];
            int mode = n < 8 ? n : 0;
            gen(in, k[kk].lo, k[kk].hi, mode);
            if (kk <= 2 || kk == 5 || kk == 6) {
                int16_t d[64];
                for (int i = 0; i < 64; i++) d[i] = (int16_t)in[i];
                if (kk == 0) ff_jpeg_fdct_islow_8(d);
                else if (kk == 1) ff_jpeg_fdct_islow_10(d);
                else if (kk == 2) ff_fdct_ifast(d);
                else ff_faandct(d);
                for (int i = 0; i < 64; i++) out[i] = d[i];
            } else if (kk == 3) {
                tj_fdct_islow_8(in, out);
            } else if (kk == 4) {
                tj_fdct_islow_12(in, out);
            } else if (kk == 7) {
                tj_fdct_islow_8i(in, out);
            } else if (kk == 8) {
                tj_fdct_ifast_8(in, out);
            } else if (kk == 9) {
                tj_fdct_ifast_12(in, out);
            } else {
                tj_fdct_float(in, out);
            }
            print_block(k[kk].name, in, out);
        }
    }
    return 0;
}
