//! Pure 2D flex/grid layout arithmetic — Qt-style two-pass measure/arrange.
//!
//! Extracted from `wgpu-widgets-core`'s `layout` module so consumers that must
//! stay free of any GPU dependency (e.g. `nodes-core` in the `nodes-rs`
//! workspace) can still share the exact same row/column layout math instead of
//! re-deriving their own pixel offsets. `wgpu-widgets-core` depends on this
//! crate and re-exports these types at the same public paths it always has —
//! existing consumers of `wgpu-widgets-core`/`wgpu-widgets` see no API change.

pub mod algo;
pub mod flex;
pub mod geometry;
pub mod resolve;
pub mod spacing;

pub use algo::{Rect, SizeHint};
pub use flex::{arrange_hbox, arrange_vbox, measure_hbox, measure_vbox, FlexChildSpec};
pub use geometry::{Align, AxisPos, AxisSize, FlexItem, Geometry, Padding};
pub use resolve::{align_cross, resolve_axis_pos, resolve_axis_size, resolve_axis_size_hint, resolve_geometry};
pub use spacing::Margins;
