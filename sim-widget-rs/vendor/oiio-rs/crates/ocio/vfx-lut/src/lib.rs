//! # vfx-lut
//!
//! Look-Up Table (LUT) types and operations for VFX color pipelines.
//!
//! This crate provides data structures for 1D and 3D LUTs, commonly used
//! for color grading, display calibration, and color space conversions.
//!
//! # LUT Types
//!
//! - [`Lut1D`] - 1-dimensional lookup table (per-channel curves)
//! - [`Lut3D`] - 3-dimensional lookup table (full RGB cube)
//!
//! # Supported Formats
//!
//! - `.cube` - Adobe/Resolve LUT format (parsing in vfx-io)
//! - `.clf` - Academy Common LUT Format ([`clf`] module)
//! - `.spi1d` / `.spi3d` - Sony Pictures Imageworks ([`spi`] module)
//!
//! # Usage
//!
//! ```rust
//! use vfx_lut::{Lut1D, Lut3D, Interpolation};
//!
//! // Create a 1D LUT (e.g., gamma curve)
//! let lut = Lut1D::gamma(1024, 2.2);
//! let output = lut.apply(0.5);
//!
//! // Create a 3D LUT (e.g., color grade)
//! let lut = Lut3D::identity(33);
//! let rgb = lut.apply([0.5, 0.3, 0.2]);
//! ```
//!
//! # Interpolation
//!
//! - 1D LUTs: Linear interpolation
//! - 3D LUTs: Trilinear or tetrahedral interpolation
//!
//! # Dependencies
//!
//! - `vfx-core` - Core types
//! - `thiserror` - Error handling
//!
//! # Used By
//!
//! - `vfx-color` - Color transformations
//! - `vfx-io` - LUT file loading

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod cdl;
pub mod clf;
pub mod csp;
pub mod cube;
pub mod discreet1dl;
mod error;
pub mod hdl;
pub mod icc;
mod interp;
pub mod iridas_itx;
pub mod iridas_look;
pub mod limits;
mod lut1d;
mod lut3d;
pub mod nuke_vf;
pub mod pandora;
pub mod spi;
pub mod spi_mtx;
pub mod threedl;
pub mod truelight;
mod xml_text;

pub use cdl::{ColorCorrection, ColorCorrectionCollection, ColorDecision, ColorDecisionList};
pub use cdl::{read_any as read_cdl_any, read_cc, read_ccc, read_cdl};
pub use cdl::{write_cc, write_ccc};
pub use clf::{ProcessList, ProcessNode, parse_ctf, read_clf, read_ctf, write_clf, write_ctf};
pub use csp::{CspFile, PreLut, PreLutChannel, parse_csp, read_csp};
pub use csp::{write_csp_1d, write_csp_3d};
pub use cube::{
    CubeFile, parse_cube, read_1d as read_cube_1d, read_3d as read_cube_3d, read_cube,
    write_1d as write_cube_1d, write_3d as write_cube_3d,
};
pub use discreet1dl::{
    BitDepth, Discreet1DLInfo, parse_1dl, read_1dl, write_1dl, write_1dl_with_options,
};
pub use error::{LutError, LutResult};
pub use hdl::{HdlFile, HdlType, parse_hdl, read_hdl, write_hdl};
pub use icc::{IccMatrixTrc, IccTrc, parse_icc, read_icc};
pub use interp::Interpolation;
pub use iridas_itx::{parse_itx, read_itx, write_itx, write_itx_file};
pub use iridas_look::{parse_look, read_look};
pub use limits::{MAX_1D_LUT_LENGTH, MAX_3D_LUT_LENGTH};
pub use lut1d::Lut1D;
pub use lut3d::{
    Lut3D, Lut3dOrder, OrderedLut3dData, interpolate_lattice, swap_red_blue_fastest_order,
};
pub use nuke_vf::{VfFile, parse_vf, read_vf};
pub use pandora::{parse_mga, read_mga};
pub use spi::{read_spi1d, read_spi3d, write_spi1d, write_spi3d};
pub use spi_mtx::{SpiMatrix, parse_spimtx, read_spimtx, write_spimtx};
pub use threedl::{parse_3dl, read_3dl, write_3dl, write_3dl_with_depth};
pub use truelight::{TruelightFile, parse_cub, read_cub, write_cub};
