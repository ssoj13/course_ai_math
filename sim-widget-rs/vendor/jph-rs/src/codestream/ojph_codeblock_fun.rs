//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codeblock_fun.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codeblock_fun.cpp`

use core::ffi::c_void;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::codestream::ojph_codestream_avx::avx_mem_clear;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::codestream::ojph_codestream_avx2::{
    avx2_find_max_val32, avx2_find_max_val64, avx2_irv_tx_from_cb32, avx2_irv_tx_to_cb32,
    avx2_rev_tx_from_cb32, avx2_rev_tx_from_cb64, avx2_rev_tx_to_cb32, avx2_rev_tx_to_cb64,
};
use crate::codestream::ojph_codestream_gen::{
    gen_find_max_val32, gen_find_max_val64, gen_irv_tx_from_cb32, gen_irv_tx_from_cb64,
    gen_irv_tx_to_cb32, gen_mem_clear, gen_rev_tx_from_cb32, gen_rev_tx_from_cb64,
    gen_rev_tx_to_cb32, gen_rev_tx_to_cb64,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::codestream::ojph_codestream_sse::sse_mem_clear;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::codestream::ojph_codestream_sse2::{
    sse2_find_max_val32, sse2_find_max_val64, sse2_irv_tx_from_cb32, sse2_irv_tx_to_cb32,
    sse2_rev_tx_from_cb32, sse2_rev_tx_from_cb64, sse2_rev_tx_to_cb32, sse2_rev_tx_to_cb64,
};
#[cfg(target_arch = "wasm32")]
use crate::codestream::ojph_codestream_wasm::{
    wasm_find_max_val32, wasm_find_max_val64, wasm_irv_tx_from_cb32, wasm_irv_tx_to_cb32,
    wasm_mem_clear, wasm_rev_tx_from_cb32, wasm_rev_tx_from_cb64, wasm_rev_tx_to_cb32,
    wasm_rev_tx_to_cb64,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_decoder_avx2::ojph_decode_codeblock_avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_decoder_ssse3::ojph_decode_codeblock_ssse3;
#[cfg(target_arch = "wasm32")]
use crate::coding::ojph_block_decoder_wasm::ojph_decode_codeblock_wasm;
use crate::coding::ojph_block_decoder32::ojph_decode_codeblock32;
use crate::coding::ojph_block_decoder64::ojph_decode_codeblock64;
use crate::coding::ojph_block_encoder::{
    initialize_block_encoder_tables, initialize_block_encoder_tables_avx2,
    initialize_block_encoder_tables_avx512, ojph_encode_codeblock32, ojph_encode_codeblock64,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_encoder_avx2::ojph_encode_codeblock_avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_encoder_avx512::ojph_encode_codeblock_avx512;
use crate::openjph::ojph_arch::{
    X86_CPU_EXT_LEVEL_AVX, X86_CPU_EXT_LEVEL_AVX2, X86_CPU_EXT_LEVEL_AVX512, X86_CPU_EXT_LEVEL_SSE,
    X86_CPU_EXT_LEVEL_SSE2, X86_CPU_EXT_LEVEL_SSSE3, force_encode_generic, force_tx_sse2,
    get_cpu_ext_level,
};
use crate::openjph::ojph_defs::{Ui32, Ui64};
use crate::openjph::ojph_mem::{CodedLists, mem_elastic_allocator};

pub type MemClearFun = unsafe fn(*mut c_void, usize);
pub type FindMaxValFun32 = unsafe fn(*mut Ui32) -> Ui32;
pub type FindMaxValFun64 = unsafe fn(*mut Ui64) -> Ui64;
pub type TxToCbFun32 = unsafe fn(*const c_void, *mut Ui32, Ui32, f32, Ui32, *mut Ui32);
pub type TxToCbFun64 = unsafe fn(*const c_void, *mut Ui64, Ui32, f32, Ui32, *mut Ui64);
pub type TxFromCbFun32 = unsafe fn(*const Ui32, *mut c_void, Ui32, f32, Ui32);
pub type TxFromCbFun64 = unsafe fn(*const Ui64, *mut c_void, Ui32, f32, Ui32);
pub type CbDecoderFun32 =
    unsafe fn(*mut u8, *mut Ui32, Ui32, Ui32, Ui32, Ui32, Ui32, Ui32, Ui32, bool) -> bool;
pub type CbDecoderFun64 =
    unsafe fn(*mut u8, *mut Ui64, Ui32, Ui32, Ui32, Ui32, Ui32, Ui32, Ui32, bool) -> bool;
pub type CbEncoderFun32 = unsafe fn(
    *mut Ui32,
    Ui32,
    Ui32,
    Ui32,
    Ui32,
    Ui32,
    &mut [Ui32; 2],
    *mut mem_elastic_allocator,
    &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()>;
pub type CbEncoderFun64 = unsafe fn(
    *mut Ui64,
    Ui32,
    Ui32,
    Ui32,
    Ui32,
    Ui32,
    &mut [Ui32; 2],
    *mut mem_elastic_allocator,
    &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodeblockFun {
    pub mem_clear: MemClearFun,
    pub find_max_val32: FindMaxValFun32,
    pub find_max_val64: FindMaxValFun64,
    pub tx_to_cb32: TxToCbFun32,
    pub tx_to_cb64: Option<TxToCbFun64>,
    pub tx_from_cb32: TxFromCbFun32,
    pub tx_from_cb64: Option<TxFromCbFun64>,
    pub decode_cb32: CbDecoderFun32,
    pub decode_cb64: CbDecoderFun64,
    pub encode_cb32: CbEncoderFun32,
    pub encode_cb64: CbEncoderFun64,
}

impl Default for CodeblockFun {
    fn default() -> Self {
        Self {
            mem_clear: gen_mem_clear,
            find_max_val32: gen_find_max_val32,
            find_max_val64: gen_find_max_val64,
            tx_to_cb32: gen_irv_tx_to_cb32,
            tx_to_cb64: None,
            tx_from_cb32: gen_irv_tx_from_cb32,
            tx_from_cb64: None,
            decode_cb32: ojph_decode_codeblock32,
            decode_cb64: ojph_decode_codeblock64,
            encode_cb32: ojph_encode_codeblock32,
            encode_cb64: ojph_encode_codeblock64,
        }
    }
}

impl CodeblockFun {
    /// Select the per-tier kernels. `trunc_quant` (irreversible only) pins the forward quantizer to
    /// the generic truncating converter on EVERY tier: the SSE2/AVX2 converters round to nearest
    /// (as OpenJPH's C++ does) while the scalar one truncates (the T.800 dead-zone quantizer), so
    /// without the pin the lossy bitstream depends on the CPU.
    pub fn init(&mut self, reversible: bool, trunc_quant: bool) {
        self.init_tier(reversible);
        if !reversible && trunc_quant {
            self.tx_to_cb32 = gen_irv_tx_to_cb32;
        }
    }

    fn init_tier(&mut self, reversible: bool) {
        #[cfg(target_arch = "wasm32")]
        {
            self.decode_cb32 = ojph_decode_codeblock_wasm;
            self.find_max_val32 = wasm_find_max_val32;
            self.mem_clear = wasm_mem_clear;
            if reversible {
                self.tx_to_cb32 = wasm_rev_tx_to_cb32;
                self.tx_from_cb32 = wasm_rev_tx_from_cb32;
            } else {
                self.tx_to_cb32 = wasm_irv_tx_to_cb32;
                self.tx_from_cb32 = wasm_irv_tx_from_cb32;
            }
            self.encode_cb32 = ojph_encode_codeblock32;

            self.decode_cb64 = ojph_decode_codeblock64;
            self.find_max_val64 = wasm_find_max_val64;
            if reversible {
                self.tx_to_cb64 = Some(wasm_rev_tx_to_cb64);
                self.tx_from_cb64 = Some(wasm_rev_tx_from_cb64);
            } else {
                self.tx_to_cb64 = None;
                // OpenJPH v0.27+ fix: previously `None` here (matching the C++ `NULL` that
                // shipped in v0.26.3), silently miscoded any 64-bit/irreversible codeblock
                // via `resolve_tx_from_cb64`'s reversible fallback. Upstream now dispatches
                // a real function; see `gen_irv_tx_from_cb64`.
                self.tx_from_cb64 = Some(gen_irv_tx_from_cb64);
            }
            self.encode_cb64 = ojph_encode_codeblock64;

            let result = initialize_block_encoder_tables();
            debug_assert!(result);
            return;
        }

        self.decode_cb32 = ojph_decode_codeblock32;
        self.find_max_val32 = gen_find_max_val32;
        self.mem_clear = gen_mem_clear;
        if reversible {
            self.tx_to_cb32 = gen_rev_tx_to_cb32;
            self.tx_from_cb32 = gen_rev_tx_from_cb32;
        } else {
            self.tx_to_cb32 = gen_irv_tx_to_cb32;
            self.tx_from_cb32 = gen_irv_tx_from_cb32;
        }
        self.encode_cb32 = ojph_encode_codeblock32;

        self.decode_cb64 = ojph_decode_codeblock64;
        self.find_max_val64 = gen_find_max_val64;
        if reversible {
            self.tx_to_cb64 = Some(gen_rev_tx_to_cb64);
            self.tx_from_cb64 = Some(gen_rev_tx_from_cb64);
        } else {
            self.tx_to_cb64 = None;
            // OpenJPH v0.27+ fix: see the wasm branch above.
            self.tx_from_cb64 = Some(gen_irv_tx_from_cb64);
        }
        self.encode_cb64 = ojph_encode_codeblock64;

        let result = initialize_block_encoder_tables();
        debug_assert!(result);

        let cpu_ext_level = get_cpu_ext_level();
        let _ = cpu_ext_level;

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if cpu_ext_level >= X86_CPU_EXT_LEVEL_SSE {
                self.mem_clear = sse_mem_clear;
            }

            if cpu_ext_level >= X86_CPU_EXT_LEVEL_SSE2 {
                self.find_max_val32 = sse2_find_max_val32;
                self.find_max_val64 = sse2_find_max_val64;
                if reversible {
                    self.tx_to_cb32 = sse2_rev_tx_to_cb32;
                    self.tx_from_cb32 = sse2_rev_tx_from_cb32;
                    self.tx_to_cb64 = Some(sse2_rev_tx_to_cb64);
                    self.tx_from_cb64 = Some(sse2_rev_tx_from_cb64);
                } else {
                    self.tx_to_cb32 = sse2_irv_tx_to_cb32;
                    self.tx_from_cb32 = sse2_irv_tx_from_cb32;
                    self.tx_to_cb64 = None;
                    // OpenJPH v0.27+ fix: see the wasm branch above.
                    self.tx_from_cb64 = Some(gen_irv_tx_from_cb64);
                }
            }

            if cpu_ext_level >= X86_CPU_EXT_LEVEL_SSSE3 {
                self.decode_cb32 = ojph_decode_codeblock_ssse3;
            }

            if cpu_ext_level >= X86_CPU_EXT_LEVEL_AVX {
                self.mem_clear = avx_mem_clear;
            }

            if cpu_ext_level >= X86_CPU_EXT_LEVEL_AVX2 {
                self.decode_cb32 = ojph_decode_codeblock_avx2;
                self.find_max_val32 = avx2_find_max_val32;
                self.find_max_val64 = avx2_find_max_val64;
                if reversible {
                    self.tx_to_cb32 = avx2_rev_tx_to_cb32;
                    self.tx_from_cb32 = avx2_rev_tx_from_cb32;
                    self.tx_to_cb64 = Some(avx2_rev_tx_to_cb64);
                    self.tx_from_cb64 = Some(avx2_rev_tx_from_cb64);
                } else {
                    self.tx_to_cb32 = avx2_irv_tx_to_cb32;
                    self.tx_from_cb32 = avx2_irv_tx_from_cb32;
                    self.tx_to_cb64 = None;
                    // OpenJPH v0.27+ fix: see the wasm branch above.
                    self.tx_from_cb64 = Some(gen_irv_tx_from_cb64);
                }
                self.encode_cb32 = ojph_encode_codeblock_avx2;
                let result = initialize_block_encoder_tables_avx2();
                debug_assert!(result);

                if force_encode_generic() {
                    self.encode_cb32 = ojph_encode_codeblock32;
                }
                if force_tx_sse2() {
                    if reversible {
                        self.tx_to_cb32 = sse2_rev_tx_to_cb32;
                        self.tx_from_cb32 = sse2_rev_tx_from_cb32;
                        self.tx_to_cb64 = Some(sse2_rev_tx_to_cb64);
                        self.tx_from_cb64 = Some(sse2_rev_tx_from_cb64);
                    } else {
                        self.tx_to_cb32 = sse2_irv_tx_to_cb32;
                        self.tx_from_cb32 = sse2_irv_tx_from_cb32;
                        self.tx_to_cb64 = None;
                        // OpenJPH v0.27+ fix: see the wasm branch above.
                        self.tx_from_cb64 = Some(gen_irv_tx_from_cb64);
                    }
                }
            }

            #[cfg(target_arch = "x86_64")]
            if cpu_ext_level >= X86_CPU_EXT_LEVEL_AVX512 {
                self.encode_cb32 = ojph_encode_codeblock_avx512;
                let result = initialize_block_encoder_tables_avx512();
                debug_assert!(result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `trunc_quant` must pin the irreversible forward quantizer to the scalar truncating kernel on
    /// whatever tier was detected, and leave reversible/decode kernels alone.
    #[test]
    fn trunc_quant_pins_scalar_forward_quantizer() {
        let mut f = CodeblockFun::default();
        f.init(false, true);

        // Values with fraction >= .5: truncating and rounding kernels disagree on them.
        let src = [1.6f32, -2.7, 3.5, 0.4, 5.9, -0.6, 7.5, 8.5];
        let mut out = [0u32; 8];
        let mut max = [0u32; 1];
        unsafe { (f.tx_to_cb32)(src.as_ptr() as *const _, out.as_mut_ptr(), 31, 1.0, 8, max.as_mut_ptr()) };
        let mags: Vec<u32> = out.iter().map(|v| v & 0x7FFF_FFFF).collect();
        assert_eq!(mags, [1, 2, 3, 0, 5, 0, 7, 8]);
    }
}
