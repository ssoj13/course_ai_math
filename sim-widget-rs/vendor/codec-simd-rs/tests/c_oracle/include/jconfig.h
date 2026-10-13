/* Minimal stand-in for the CMake-generated jconfig.h (TEST-ONLY): just enough for jfdctint.c to compile. */
#define JPEG_LIB_VERSION  62
#define LIBJPEG_TURBO_VERSION  3.2.1
#define LIBJPEG_TURBO_VERSION_NUMBER  3002001
#define MEM_SRCDST_SUPPORTED  1
#ifndef BITS_IN_JSAMPLE
#define BITS_IN_JSAMPLE  8
#endif
#if BITS_IN_JSAMPLE == 8 && defined(KAT_SIMD)
#define WITH_SIMD 1
#endif
#ifdef _WIN32
#define HAVE_BOOLEAN
typedef unsigned char boolean;
#ifndef __RPCNDR_H__
#endif
#define XMD_H
typedef short INT16;
typedef signed int INT32;
#endif
