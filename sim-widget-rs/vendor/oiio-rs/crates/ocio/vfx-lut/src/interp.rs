//! Interpolation methods for LUT evaluation.

/// Interpolation method for LUT evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpolation {
    /// Nearest neighbor (no interpolation).
    Nearest,

    /// Linear interpolation (1D) / Trilinear (3D).
    ///
    /// Default method, good balance of quality and speed.
    #[default]
    Linear,

    /// Tetrahedral interpolation (3D only).
    ///
    /// Higher quality than trilinear, especially for smooth gradients.
    Tetrahedral,
}
