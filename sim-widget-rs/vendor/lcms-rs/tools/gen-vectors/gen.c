// Golden-vector generator: runs the REAL Little-CMS (lcms2 2.19, same version as our
// _ref submodule) and dumps deterministic outputs that the Rust port is diffed against.
// Build: cl /I<vcpkg>/include gen.c <vcpkg>/lib/lcms2.lib
// Output: a text file on argv[1]; each line is "KEY: v0 v1 v2 ..." parsed by the Rust test.
#include <stdio.h>
#include <string.h>
#include "lcms2.h"

static FILE* g;

// The RGB test grid (5^3 = 125 triples): 0,64,128,192,255 per channel.
static const int GRID[5] = {0, 64, 128, 192, 255};

static void dump_transform_rgb8_to_rgb8(const char* key, cmsHTRANSFORM t) {
    for (int r = 0; r < 5; r++)
        for (int gg = 0; gg < 5; gg++)
            for (int b = 0; b < 5; b++) {
                cmsUInt8Number in[3] = {(cmsUInt8Number)GRID[r], (cmsUInt8Number)GRID[gg], (cmsUInt8Number)GRID[b]};
                cmsUInt8Number out[3] = {0,0,0};
                cmsDoTransform(t, in, out, 1);
                fprintf(g, "%s %d %d %d: %d %d %d\n", key, in[0], in[1], in[2], out[0], out[1], out[2]);
            }
}

static void dump_transform_rgb8_to_cmyk8(const char* key, cmsHTRANSFORM t) {
    for (int r = 0; r < 5; r++)
        for (int gg = 0; gg < 5; gg++)
            for (int b = 0; b < 5; b++) {
                cmsUInt8Number in[3] = {(cmsUInt8Number)GRID[r], (cmsUInt8Number)GRID[gg], (cmsUInt8Number)GRID[b]};
                cmsUInt8Number out[4] = {0,0,0,0};
                cmsDoTransform(t, in, out, 1);
                fprintf(g, "%s %d %d %d: %d %d %d %d\n", key, in[0], in[1], in[2], out[0], out[1], out[2], out[3]);
            }
}

static void dump_transform_rgb8_to_lab16(const char* key, cmsHTRANSFORM t) {
    for (int r = 0; r < 5; r++)
        for (int gg = 0; gg < 5; gg++)
            for (int b = 0; b < 5; b++) {
                cmsUInt8Number in[3] = {(cmsUInt8Number)GRID[r], (cmsUInt8Number)GRID[gg], (cmsUInt8Number)GRID[b]};
                cmsUInt16Number out[3] = {0,0,0};
                cmsDoTransform(t, in, out, 1);
                fprintf(g, "%s %d %d %d: %u %u %u\n", key, in[0], in[1], in[2], out[0], out[1], out[2]);
            }
}

int main(int argc, char** argv) {
    if (argc < 3) { fprintf(stderr, "usage: gen <out.txt> <testbed_dir>\n"); return 1; }
    g = fopen(argv[1], "wb");
    if (!g) { fprintf(stderr, "cannot open %s\n", argv[1]); return 1; }
    char path[1024];

    cmsHPROFILE sRGB = cmsCreate_sRGBProfile();
    cmsHPROFILE lab4 = cmsCreateLab4Profile(NULL);
    cmsHPROFILE xyz  = cmsCreateXYZProfile();

    // 1) sRGB -> sRGB, RGB8, all four intents, BPC off.
    for (cmsUInt32Number intent = 0; intent <= 3; intent++) {
        cmsHTRANSFORM t = cmsCreateTransform(sRGB, TYPE_RGB_8, sRGB, TYPE_RGB_8, intent, 0);
        char key[64]; sprintf(key, "srgb_srgb_i%u", intent);
        dump_transform_rgb8_to_rgb8(key, t);
        cmsDeleteTransform(t);
    }
    // 2) sRGB -> sRGB with BPC (relative colorimetric).
    {
        cmsHTRANSFORM t = cmsCreateTransform(sRGB, TYPE_RGB_8, sRGB, TYPE_RGB_8,
                                             INTENT_RELATIVE_COLORIMETRIC, cmsFLAGS_BLACKPOINTCOMPENSATION);
        dump_transform_rgb8_to_rgb8("srgb_srgb_bpc", t);
        cmsDeleteTransform(t);
    }
    // 3) sRGB -> Lab4, RGB8 -> Lab16 (RGB to PCS via matrix-shaper).
    {
        cmsHTRANSFORM t = cmsCreateTransform(sRGB, TYPE_RGB_8, lab4, TYPE_Lab_16,
                                             INTENT_RELATIVE_COLORIMETRIC, 0);
        dump_transform_rgb8_to_lab16("srgb_lab16", t);
        cmsDeleteTransform(t);
    }
    // 4) sRGB -> XYZ profile, RGB8 -> Lab16 for a second matrix-shaper path.
    {
        cmsHTRANSFORM t = cmsCreateTransform(sRGB, TYPE_RGB_8, xyz, TYPE_XYZ_16,
                                             INTENT_RELATIVE_COLORIMETRIC, 0);
        if (t) { dump_transform_rgb8_to_lab16("srgb_xyz16", t); cmsDeleteTransform(t); }
    }
    // 5) sRGB -> CMYK printer profile (full CLUT), RGB8 -> CMYK8, perceptual.
    snprintf(path, sizeof(path), "%s/test1.icc", argv[2]);
    cmsHPROFILE cmyk = cmsOpenProfileFromFile(path, "r");
    if (cmyk) {
        cmsHTRANSFORM t = cmsCreateTransform(sRGB, TYPE_RGB_8, cmyk, TYPE_CMYK_8,
                                             INTENT_PERCEPTUAL, 0);
        if (t) { dump_transform_rgb8_to_cmyk8("srgb_cmyk8_perc", t); cmsDeleteTransform(t); }
        // 6) CMYK -> sRGB back (RGB8), relative colorimetric.
        cmsHTRANSFORM t2 = cmsCreateTransform(cmyk, TYPE_CMYK_8, sRGB, TYPE_RGB_8,
                                              INTENT_RELATIVE_COLORIMETRIC, 0);
        cmsCloseProfile(cmyk);
    }

    // 7) Lab V4 encode/decode: Float2LabEncoded / LabEncoded2Float round values.
    {
        double labs[][3] = {{0,0,0},{50,0,0},{100,0,0},{53.2,80.1,67.2},{87.7,-86.2,83.2},{32.3,79.2,-107.9}};
        for (int i = 0; i < 6; i++) {
            cmsCIELab L = {labs[i][0], labs[i][1], labs[i][2]};
            cmsUInt16Number enc[3];
            cmsFloat2LabEncoded(enc, &L);
            fprintf(g, "lab_v4_enc %.4f %.4f %.4f: %u %u %u\n", L.L, L.a, L.b, enc[0], enc[1], enc[2]);
        }
    }
    // 8) XYZ encode.
    {
        double xyzs[][3] = {{0,0,0},{0.9642,1.0,0.8249},{0.5,0.5,0.5},{0.1,0.2,0.3}};
        for (int i = 0; i < 4; i++) {
            cmsCIEXYZ X = {xyzs[i][0], xyzs[i][1], xyzs[i][2]};
            cmsUInt16Number enc[3];
            cmsFloat2XYZEncoded(enc, &X);
            fprintf(g, "xyz_enc %.6f %.6f %.6f: %u %u %u\n", X.X, X.Y, X.Z, enc[0], enc[1], enc[2]);
        }
    }
    // 9) sRGB tone curve (parametric type 4) eval at 16-bit points.
    {
        cmsFloat64Number params[5] = {2.4, 1.0/1.055, 0.055/1.055, 1.0/12.92, 0.04045};
        cmsToneCurve* c = cmsBuildParametricToneCurve(NULL, 4, params);
        for (int i = 0; i <= 16; i++) {
            cmsUInt16Number in = (cmsUInt16Number)(i * 4095);
            cmsUInt16Number out = cmsEvalToneCurve16(c, in);
            fprintf(g, "srgb_curve16 %u: %u\n", in, out);
        }
        cmsFreeToneCurve(c);
    }
    // 10) DeltaE 2000 for pairs.
    {
        cmsCIELab a = {50, 2.6772, -79.7751}, b = {50, 0, -82.7485};
        fprintf(g, "de2000 %.10f\n", cmsCIE2000DeltaE(&a, &b, 1, 1, 1));
        cmsCIELab c1 = {63.0109, -31.0961, -5.8663}, c2 = {62.8187, -29.7946, -4.0864};
        fprintf(g, "de2000 %.10f\n", cmsCIE2000DeltaE(&c1, &c2, 1, 1, 1));
    }

    cmsCloseProfile(sRGB);
    cmsCloseProfile(lab4);
    cmsCloseProfile(xyz);
    fclose(g);
    fprintf(stderr, "golden vectors written to %s\n", argv[1]);
    return 0;
}
