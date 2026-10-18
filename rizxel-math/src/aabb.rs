//! Axis-aligned bounding box built on `glam`.

use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};

use crate::BoundSize;

/// Axis-aligned bounding box in world space. `min` is inclusive, `max` exclusive
/// when used for voxel containment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// Creates a box, normalizing so that `min <= max` on every axis.
    pub fn new(a: Vec3, b: Vec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Box spanning `[0, bounds)` — the editable model volume.
    pub fn from_bounds(bounds: BoundSize) -> Self {
        Self::new(Vec3::ZERO, bounds.0.as_vec3())
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Point containment (inclusive on both ends).
    pub fn contains_point(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// Voxel containment: the voxel cell `[v, v+1)` lies fully inside the box.
    pub fn contains_voxel(&self, v: IVec3) -> bool {
        let lo = v.as_vec3();
        let hi = lo + Vec3::ONE;
        lo.cmpge(self.min).all() && hi.cmple(self.max).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bounds_and_containment() {
        let b = Aabb::from_bounds(BoundSize::new(4, 2, 8));
        assert_eq!(b.size(), Vec3::new(4.0, 2.0, 8.0));
        assert!(b.contains_voxel(IVec3::new(3, 1, 7)));
        assert!(!b.contains_voxel(IVec3::new(4, 0, 0)));
        assert!(!b.contains_voxel(IVec3::new(-1, 0, 0)));
    }

    #[test]
    fn new_normalizes() {
        let b = Aabb::new(Vec3::ONE, Vec3::ZERO);
        assert_eq!(b.min, Vec3::ZERO);
        assert_eq!(b.center(), Vec3::splat(0.5));
    }
}
