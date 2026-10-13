/* Minimal stand-in for the CMake-generated jconfigint.h (TEST-ONLY). */
#define BUILD  "kat"
#define HIDDEN
#undef inline
#define INLINE  __inline
#define THREAD_LOCAL  __declspec(thread)
#define PACKAGE_NAME  "libjpeg-turbo"
#define VERSION  "3.2.1"
#define SIZEOF_SIZE_T  8
#define FALLTHROUGH
#ifndef BITS_IN_JSAMPLE
#define BITS_IN_JSAMPLE  8
#endif
#if BITS_IN_JSAMPLE == 8 && defined(KAT_SIMD)
#ifndef WITH_SIMD
#define WITH_SIMD 1
#endif
#endif
