//! Internal scalar math: deterministic libm by default; native f32 operations for CUDA.
//! The cuda-math feature avoids libm's x86 SSE sqrt intrinsic during device lowering.
#![allow(missing_docs)]

#[inline(always)]
pub(crate) fn sqrtf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.sqrt()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::sqrtf(x)
    }
}

#[inline(always)]
pub(crate) fn powf(x: f32, y: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.powf(y)
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::powf(x, y)
    }
}

#[inline(always)]
pub(crate) fn expf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.exp()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::expf(x)
    }
}

/// `(sin x, cos x)` of one angle.
///
/// Device builds (`cuda-math`) use [`cuda_device::math::sin_cos`]: libdevice's own bits for
/// `|x| < 105615` without the Payne-Hanek reduction, whose seven-word table would put every
/// inlined sine and cosine of the BSDF into local memory. Every angle this crate takes the
/// sine or cosine of is bounded far below that: sampled azimuths `2 pi u`, anisotropy
/// rotations of a few turns, `atan` results, Burley profile angles in `[-pi, 2 pi]` and
/// thin-film phases reduced by [`crate::fresnel::reduce_phase`]. One reduction serves both
/// results, so a sampled direction pays for one.
#[inline(always)]
pub(crate) fn sin_cos(x: f32) -> (f32, f32) {
    #[cfg(feature = "cuda-math")]
    {
        cuda_device::math::sin_cos(x)
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        (libm::sinf(x), libm::cosf(x))
    }
}

#[inline(always)]
pub(crate) fn sinf(x: f32) -> f32 {
    sin_cos(x).0
}

#[inline(always)]
pub(crate) fn cosf(x: f32) -> f32 {
    sin_cos(x).1
}

#[inline(always)]
pub(crate) fn tanf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.tan()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::tanf(x)
    }
}

#[inline(always)]
pub(crate) fn acosf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.acos()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::acosf(x)
    }
}

#[inline(always)]
pub(crate) fn atanf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.atan()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::atanf(x)
    }
}

#[inline(always)]
pub(crate) fn atan2f(y: f32, x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        y.atan2(x)
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::atan2f(y, x)
    }
}

#[inline(always)]
pub(crate) fn floorf(x: f32) -> f32 {
    #[cfg(feature = "cuda-math")]
    {
        x.floor()
    }
    #[cfg(not(feature = "cuda-math"))]
    {
        libm::floorf(x)
    }
}
