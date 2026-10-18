//! Mouse-to-voxel raycasting.
//!
//! Phase 2 provides the ray type and an analytic ray/AABB test that will be
//! used to clip rays against the model bounds. The voxel DDA traversal is
//! added in Phase 5.

use glam::Vec3;
use rizxel_math::Aabb;

/// A ray in world space. `dir` is expected to be normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, dir: Vec3) -> Self {
        Self {
            origin,
            dir: dir.normalize_or_zero(),
        }
    }

    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }

    /// Slab test. Returns the entry distance `t >= 0` if the ray hits `aabb`.
    pub fn intersect_aabb(&self, aabb: &Aabb) -> Option<f32> {
        let inv = self.dir.recip();
        let t0 = (aabb.min - self.origin) * inv;
        let t1 = (aabb.max - self.origin) * inv;
        let t_near = t0.min(t1).max_element();
        let t_far = t0.max(t1).min_element();

        (t_far >= t_near.max(0.0)).then_some(t_near.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_and_misses_box() {
        let b = Aabb::new(Vec3::ZERO, Vec3::ONE);
        let hit = Ray::new(Vec3::new(0.5, 0.5, -2.0), Vec3::Z);
        assert_eq!(hit.intersect_aabb(&b), Some(2.0));

        let miss = Ray::new(Vec3::new(5.0, 0.5, -2.0), Vec3::Z);
        assert_eq!(miss.intersect_aabb(&b), None);

        let inside = Ray::new(Vec3::splat(0.5), Vec3::X);
        assert_eq!(inside.intersect_aabb(&b), Some(0.0));
    }
}
