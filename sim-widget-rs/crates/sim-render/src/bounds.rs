//! Actual bounds from transformed geometric points.
use crate::{Result, error::invalid};
use glam::{DMat4, DVec3};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: DVec3,
    pub max: DVec3,
}
impl Bounds {
    pub fn from_points(points: impl IntoIterator<Item = DVec3>) -> Result<Self> {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for point in points {
            if !point.is_finite() {
                return Err(invalid("bounds", "nonfinite point"));
            }
            min = min.min(point);
            max = max.max(point);
        }
        if !min.is_finite() {
            return Err(invalid("bounds", "empty geometry"));
        }
        Ok(Self { min, max })
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
    pub fn center(self) -> DVec3 {
        (self.min + self.max) * 0.5
    }
    pub fn corners(self) -> [DVec3; 8] {
        std::array::from_fn(|i| {
            DVec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        })
    }
    pub fn transformed(self, world: DMat4) -> Result<Self> {
        Self::from_points(self.corners().map(|p| world.transform_point3(p)))
    }
}
