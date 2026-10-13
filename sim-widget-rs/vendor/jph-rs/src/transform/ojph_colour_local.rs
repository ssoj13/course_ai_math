//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_local.h`

use crate::openjph::ojph_defs::{Si64, Ui32};
use crate::openjph::ojph_mem::LineBuf;

#[allow(non_camel_case_types)]
pub struct CT_CNST;

#[allow(non_upper_case_globals)]
impl CT_CNST {
    pub const ALPHA_RF: f32 = 0.299f32;
    pub const ALPHA_GF: f32 = 0.587f32;
    pub const ALPHA_BF: f32 = 0.114f32;
    pub const BETA_CbF: f32 = (0.5f64 / (1.0f64 - Self::ALPHA_BF as f64)) as f32;
    pub const BETA_CrF: f32 = (0.5f64 / (1.0f64 - Self::ALPHA_RF as f64)) as f32;
    pub const GAMMA_CR2R: f32 = (2.0f64 * (1.0f64 - Self::ALPHA_RF as f64)) as f32;
    pub const GAMMA_CB2B: f32 = (2.0f64 * (1.0f64 - Self::ALPHA_BF as f64)) as f32;
    pub const GAMMA_CR2G: f32 = (2.0f64 * Self::ALPHA_RF as f64 * (1.0f64 - Self::ALPHA_RF as f64)
        / Self::ALPHA_GF as f64) as f32;
    pub const GAMMA_CB2G: f32 = (2.0f64 * Self::ALPHA_BF as f64 * (1.0f64 - Self::ALPHA_BF as f64)
        / Self::ALPHA_GF as f64) as f32;
}

pub type RevConvert = unsafe fn(*const LineBuf, Ui32, *mut LineBuf, Ui32, Si64, Ui32);
pub type IrvConvertToFloat = unsafe fn(*const LineBuf, Ui32, *mut LineBuf, Ui32, bool, Ui32);
pub type IrvConvertToInteger = unsafe fn(*const LineBuf, *mut LineBuf, Ui32, Ui32, bool, Ui32);
pub type RctTransform = unsafe fn(
    *const LineBuf,
    *const LineBuf,
    *const LineBuf,
    *mut LineBuf,
    *mut LineBuf,
    *mut LineBuf,
    Ui32,
);
pub type IctTransform =
    unsafe fn(*const f32, *const f32, *const f32, *mut f32, *mut f32, *mut f32, Ui32);

pub use crate::transform::ojph_colour::gen_ict_backward;
pub use crate::transform::ojph_colour::gen_ict_forward;
pub use crate::transform::ojph_colour::gen_irv_convert_to_float;
pub use crate::transform::ojph_colour::gen_irv_convert_to_float_nlt_type3;
pub use crate::transform::ojph_colour::gen_irv_convert_to_integer;
pub use crate::transform::ojph_colour::gen_irv_convert_to_integer_nlt_type3;
pub use crate::transform::ojph_colour::gen_rct_backward;
pub use crate::transform::ojph_colour::gen_rct_forward;
pub use crate::transform::ojph_colour::gen_rev_convert;
pub use crate::transform::ojph_colour::gen_rev_convert_nlt_type3;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub use crate::transform::ojph_colour_sse::sse_ict_backward;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub use crate::transform::ojph_colour_sse::sse_ict_forward;
