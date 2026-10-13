//! Window display output for egui: an extended-sRGB float canvas, then one
//! transfer/gamut/dither pass into a supported surface format and colour space.
//! Hosts own windows, surfaces, persistence and the scene's colour transform.
//! The viewer backdrop (`backdrop`: what shows under the image's alpha) lives here
//! too, because it is composited in that same display space.
mod backdrop;
mod capture;
pub mod export;
mod present;
mod retire;
pub mod screenshot;
mod settings;
pub mod transfer;
pub use backdrop::*;
pub use capture::*;
pub use present::*;
pub use settings::*;
