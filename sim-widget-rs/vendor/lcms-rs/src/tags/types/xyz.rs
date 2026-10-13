//! `cmsSigXYZType` — a single encoded XYZ tristimulus (`Type_XYZ_*` in `cmstypes.c`).
//!
//! Also serves the broken `cmsCorbisBrokenXYZtype` alias (same reader/writer, different
//! type signature on disk), exactly as the C `SupportedTagTypes` table does.

use std::io::{Read, Seek, Write};

use crate::error::Result;
use crate::io::endian::{read_xyz, write_xyz};

use super::super::handler::wrong_variant;
use super::super::value::TagValue;

/// `Type_XYZ_Read` — read one `XYZNumber`.
pub(crate) fn read<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::Xyz(read_xyz(io)?))
}

/// `Type_XYZ_Write` — write one `XYZNumber`.
pub(crate) fn write<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::Xyz(xyz) => write_xyz(io, xyz),
        _ => Err(wrong_variant("XYZ")),
    }
}

#[cfg(test)]
mod tests {
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;
    use crate::tags::value::TagValue;
    use crate::types::color::CIEXYZ;

    #[test]
    fn xyz_round_trip() {
        let v = TagValue::Xyz(CIEXYZ { x: 0.9642, y: 1.0, z: 0.8249 });
        round_trip_stable(TypeHandler::Xyz, &v);
    }
}
