//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_base.h`

use crate::openjph::ojph_defs::{Ui32, Ui64};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub w: Ui32,
    pub h: Ui32,
}

impl Size {
    #[inline]
    pub const fn new(w: Ui32, h: Ui32) -> Self {
        Self { w, h }
    }

    #[inline]
    pub const fn area(&self) -> Ui64 {
        (self.w as Ui64) * (self.h as Ui64)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: Ui32,
    pub y: Ui32,
}

impl Point {
    #[inline]
    pub const fn new(x: Ui32, y: Ui32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub org: Point,
    pub siz: Size,
}
