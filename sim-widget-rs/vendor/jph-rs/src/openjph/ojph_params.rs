//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_params.h`

use crate::openjph::ojph_defs::{Ui8, Ui16, Ui32};

pub const ALL_COMPS: Ui16 = 65535;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Nonlinearity {
    OjphNltNoNlt = 0,
    OjphNltGammaStyleNlt = 1,
    OjphNltLutStyleNlt = 2,
    OjphNltBinaryComplementNlt = 3,
    OjphNltUndefined = 255,
}

impl Nonlinearity {
    #[inline]
    pub const fn from_u8(value: Ui8) -> Self {
        match value {
            0 => Self::OjphNltNoNlt,
            1 => Self::OjphNltGammaStyleNlt,
            2 => Self::OjphNltLutStyleNlt,
            3 => Self::OjphNltBinaryComplementNlt,
            _ => Self::OjphNltUndefined,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParamNltEntry {
    pub lnlt: Ui16,
    pub cnlt: Ui16,
    pub bdnlt: Ui8,
    pub tnlt: Ui8,
    pub enabled: bool,
}

impl ParamNltEntry {
    #[inline]
    pub const fn new(comp_num: Ui32) -> Self {
        Self {
            lnlt: 6,
            cnlt: comp_num as Ui16,
            bdnlt: 0,
            tnlt: Nonlinearity::OjphNltUndefined as Ui8,
            enabled: false,
        }
    }

    #[inline]
    pub fn set_nonlinear_transform(
        &mut self,
        nl_type: Ui8,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if nl_type != Nonlinearity::OjphNltNoNlt as Ui8
            && nl_type != Nonlinearity::OjphNltBinaryComplementNlt as Ui8
        {
            return Err(crate::ojph_error!(
                0x00050171,
                "Nonliearities other than type 0 (No Nonlinearity) or type  3 (Binary Binary Complement to Sign Magnitude Conversion) are not supported yet"
            ));
        }

        self.tnlt = nl_type;
        self.enabled = true;
        Ok(())
    }

    #[inline]
    pub fn get_nonlinear_transform(&self) -> Option<(Ui8, bool, Ui8)> {
        if self.enabled {
            let mut bit_depth = (self.bdnlt & 0x7F) + 1;
            bit_depth = bit_depth.min(38);
            let is_signed = (self.bdnlt & 0x80) == 0x80;
            Some((bit_depth, is_signed, self.tnlt))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CommentExchange {
    pub data: *const u8,
    pub len: Ui16,
    pub rcom: Ui16,
}

impl Default for CommentExchange {
    fn default() -> Self {
        Self {
            data: core::ptr::null(),
            len: 0,
            rcom: 0,
        }
    }
}

impl CommentExchange {
    #[inline]
    pub fn set_string(&mut self, string: &str) -> crate::openjph::ojph_message::OjphResult<()> {
        let t = string.len();
        if t > 65_531 {
            return Err(crate::ojph_error!(
                0x000500C1,
                "COM marker string length cannot be larger than 65531"
            ));
        }

        self.data = string.as_ptr();
        self.len = t as Ui16;
        self.rcom = 1;
        Ok(())
    }

    #[inline]
    pub fn set_data(&mut self, data: &[u8]) -> crate::openjph::ojph_message::OjphResult<()> {
        if data.len() > 65_531 {
            return Err(crate::ojph_error!(
                0x000500C2,
                "COM marker string length cannot be larger than 65531"
            ));
        }

        self.data = data.as_ptr();
        self.len = data.len() as Ui16;
        self.rcom = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ALL_COMPS, CommentExchange, Nonlinearity, ParamNltEntry};

    #[test]
    fn all_comps_constant_matches_reference() {
        assert_eq!(ALL_COMPS, 65535);
    }

    #[test]
    fn nonlinearity_values_match_reference() {
        assert_eq!(Nonlinearity::OjphNltNoNlt as u8, 0);
        assert_eq!(Nonlinearity::OjphNltGammaStyleNlt as u8, 1);
        assert_eq!(Nonlinearity::OjphNltLutStyleNlt as u8, 2);
        assert_eq!(Nonlinearity::OjphNltBinaryComplementNlt as u8, 3);
        assert_eq!(Nonlinearity::OjphNltUndefined as u8, 255);
    }

    #[test]
    fn param_nlt_entry_accepts_only_supported_types() {
        let mut entry = ParamNltEntry::new(ALL_COMPS as u32);
        entry.set_nonlinear_transform(Nonlinearity::OjphNltNoNlt as u8).expect("set");
        assert_eq!(entry.tnlt, Nonlinearity::OjphNltNoNlt as u8);
        entry
            .set_nonlinear_transform(Nonlinearity::OjphNltBinaryComplementNlt as u8)
            .expect("set");
        assert_eq!(entry.tnlt, Nonlinearity::OjphNltBinaryComplementNlt as u8);
    }

    #[test]
    fn comment_exchange_string_and_data_match_reference_flags() {
        let mut comment = CommentExchange::default();
        comment.set_string("hello").expect("set_string");
        assert_eq!(comment.len, 5);
        assert_eq!(comment.rcom, 1);

        comment.set_data(&[1, 2, 3]).expect("set_data");
        assert_eq!(comment.len, 3);
        assert_eq!(comment.rcom, 0);
    }

    #[test]
    fn param_nlt_entry_rejects_unsupported_type() {
        let mut entry = ParamNltEntry::new(ALL_COMPS as u32);
        // Only type 0 (No Nonlinearity) and type 3 (Binary Complement) are supported;
        // 1 (Gamma) and 2 (LUT) must error, not silently accept an unsupported type.
        assert!(entry.set_nonlinear_transform(Nonlinearity::OjphNltGammaStyleNlt as u8).is_err());
        assert!(entry.set_nonlinear_transform(Nonlinearity::OjphNltLutStyleNlt as u8).is_err());
    }

    #[test]
    fn comment_exchange_rejects_oversized_string() {
        let mut comment = CommentExchange::default();
        let too_long = "a".repeat(65_532);
        assert!(comment.set_string(&too_long).is_err());
    }

    #[test]
    fn comment_exchange_rejects_oversized_data() {
        let mut comment = CommentExchange::default();
        let too_long = vec![0u8; 65_532];
        assert!(comment.set_data(&too_long).is_err());
    }
}
