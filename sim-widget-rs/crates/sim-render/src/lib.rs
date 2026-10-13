//! Scene rendering using a caller-owned wgpu device and queue.
//! Runtime truth remains owned by sim-runtime; rendering never evaluates lesson mathematics.
#![forbid(unsafe_code)]
pub mod bounds;
pub mod camera;
pub mod capability;
pub mod error;
pub mod frame;
pub mod geometry;
pub mod gpu;
pub mod hierarchy;
pub mod interaction;
pub mod input;
pub mod bindings;
pub mod input_pointer;
pub mod picking;
pub mod material;
pub mod shader;
pub use error::{RenderError, Result};
