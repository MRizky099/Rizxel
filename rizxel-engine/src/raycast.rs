//! Mouse-to-voxel raycasting.
//!
//! Rays live in voxel space: voxel `v` occupies the unit cell `[v, v + 1)`.
//! [`raycast_grid`] clips the ray against an integer box and then walks the
//! cells it crosses with the Amanatides–Woo DDA, so the cost is proportional
//! to the number of cells crossed, never to the volume.

use glam::{IVec3, Vec3};
use rizxel_math::{Aabb, BoundSize, VoxelBox, VoxelVolume, EMPTY_VOXEL};

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
        slab(self, aabb).map(|(t, _, _)| t.max(0.0))
    }
}

/// Result of a voxel pick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The solid voxel that was hit.
    pub voxel: IVec3,
    /// Unit normal of the entered face, so `voxel + normal` is the empty
    /// cell in front of it.
    pub normal: IVec3,
    /// Ray distance to the entry point.
    pub t: f32,
}

impl RayHit {
    /// Cell in front of the hit face.
    pub fn adjacent(&self) -> IVec3 {
        self.voxel + self.normal
    }

    /// Axis index (`0 = X`, `1 = Y`, `2 = Z`) of the face normal.
    pub fn axis(&self) -> usize {
        dominant_axis(self.normal.as_vec3())
    }

    /// Coordinate of the hit face's plane along [`RayHit::axis`].
    pub fn face_coord(&self) -> i32 {
        let a = self.axis();
        self.voxel[a] + self.normal[a].max(0)
    }
}

/// Index of the largest absolute component (ties pick the lowest axis;
/// a zero vector reports `Y`).
pub fn dominant_axis(v: Vec3) -> usize {
    let a = v.abs();
    if a == Vec3::ZERO {
        1
    } else if a.x >= a.y && a.x >= a.z {
        0
    } else if a.y >= a.z {
        1
    } else {
        2
    }
}

/// Slab test returning `(t_near, t_far, entry axis)`. `t_near` may be
/// negative when the origin is inside. Zero direction components are handled
/// explicitly so no `0 * inf = NaN` can appear.
fn slab(ray: &Ray, aabb: &Aabb) -> Option<(f32, f32, usize)> {
    let mut t_near = f32::NEG_INFINITY;
    let mut t_far = f32::INFINITY;
    let mut axis = dominant_axis(ray.dir);
    for i in 0..3 {
        let (o, d) = (ray.origin[i], ray.dir[i]);
        if d == 0.0 {
            if o < aabb.min[i] || o > aabb.max[i] {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((aabb.min[i] - o) / d, (aabb.max[i] - o) / d);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if t0 > t_near {
            t_near = t0;
            axis = i;
        }
        t_far = t_far.min(t1);
    }
    (t_far >= t_near.max(0.0)).then_some((t_near, t_far, axis))
}

/// Unit vector along `axis` with the given sign.
fn axis_vec(axis: usize, sign: i32) -> IVec3 {
    let mut v = IVec3::ZERO;
    v[axis] = sign;
    v
}

/// Walks the cells of `area` crossed by `ray` and returns the first one for
/// which `solid` is true. A ray starting inside a solid cell hits it with
/// the normal facing back along the ray.
pub fn raycast_grid(
    ray: &Ray,
    area: &VoxelBox,
    mut solid: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    if !area.is_valid() || ray.dir == Vec3::ZERO || !ray.dir.is_finite() {
        return None;
    }
    let aabb = Aabb::new(area.min.as_vec3(), area.max.as_vec3());
    let (t_near, t_far, entry_axis) = slab(ray, &aabb)?;
    let t_start = t_near.max(0.0);
    let step = IVec3::new(
        ray.dir.x.signum() as i32 * (ray.dir.x != 0.0) as i32,
        ray.dir.y.signum() as i32 * (ray.dir.y != 0.0) as i32,
        ray.dir.z.signum() as i32 * (ray.dir.z != 0.0) as i32,
    );
    let mut normal = if t_near < 0.0 {
        let a = dominant_axis(ray.dir);
        axis_vec(a, -step[a])
    } else {
        axis_vec(entry_axis, -step[entry_axis])
    };
    // Floor the entry point; clamping fixes points exactly on the far faces
    // and float noise around the entry face.
    let mut cell = ray
        .at(t_start)
        .floor()
        .as_ivec3()
        .clamp(area.min, area.max - IVec3::ONE);

    let mut t_max = Vec3::INFINITY;
    let mut t_delta = Vec3::INFINITY;
    for i in 0..3 {
        let d = ray.dir[i];
        if d != 0.0 {
            let next = if d > 0.0 { cell[i] + 1 } else { cell[i] };
            t_max[i] = (next as f32 - ray.origin[i]) / d;
            t_delta[i] = 1.0 / d.abs();
        }
    }

    let mut t = t_start;
    let max_steps = area.size().element_sum() as usize + 3;
    for _ in 0..max_steps {
        if solid(cell) {
            return Some(RayHit {
                voxel: cell,
                normal,
                t,
            });
        }
        let i = if t_max.x <= t_max.y && t_max.x <= t_max.z {
            0
        } else if t_max.y <= t_max.z {
            1
        } else {
            2
        };
        if !t_max[i].is_finite() || t_max[i] > t_far + 1e-4 {
            return None;
        }
        cell[i] += step[i];
        if !area.contains(cell) {
            return None;
        }
        t = t_max[i];
        t_max[i] += t_delta[i];
        normal = axis_vec(i, -step[i]);
    }
    None
}

/// First non-empty voxel of `volume` along `ray`. The ray is clipped to the
/// occupied bounds first, so empty space around the model costs nothing.
pub fn raycast_volume(ray: &Ray, volume: &VoxelVolume) -> Option<RayHit> {
    let (lo, hi) = volume.occupied_bounds()?;
    let area = VoxelBox { min: lo, max: hi };
    raycast_grid(ray, &area, |p| volume.get(p) != EMPTY_VOXEL)
}

/// Point where `ray` crosses the axis-aligned plane `p[axis] = coord`, if it
/// lies in front of the origin.
pub fn raycast_face(ray: &Ray, axis: usize, coord: f32) -> Option<Vec3> {
    let d = ray.dir[axis];
    if d == 0.0 {
        return None;
    }
    let t = (coord - ray.origin[axis]) / d;
    (t >= 0.0 && t.is_finite()).then(|| ray.at(t))
}

/// Ground pick: the floor `y = 0` inside the bounds' XZ footprint, reported
/// as a hit on the virtual voxel row `y = -1` with an up normal, so
/// `hit.adjacent()` is the floor cell.
pub fn raycast_floor(ray: &Ray, bounds: BoundSize) -> Option<RayHit> {
    if ray.dir.y >= 0.0 {
        return None;
    }
    let p = raycast_face(ray, 1, 0.0)?;
    let cell = IVec3::new(p.x.floor() as i32, -1, p.z.floor() as i32);
    let size = bounds.0.as_ivec3();
    let inside = cell.x >= 0 && cell.z >= 0 && cell.x < size.x && cell.z < size.z;
    inside.then(|| RayHit {
        voxel: cell,
        normal: IVec3::Y,
        t: (p - ray.origin).length(),
    })
}

/// Voxel pick against `solid` inside `area` (usually the occupied bounds),
/// falling back to the floor of the model bounds.
pub fn pick(
    ray: &Ray,
    bounds: BoundSize,
    area: Option<VoxelBox>,
    solid: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    area.and_then(|a| raycast_grid(ray, &a, solid))
        .or_else(|| raycast_floor(ray, bounds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(n: u32) -> VoxelVolume {
        VoxelVolume::new(BoundSize::new(n, n, n))
    }

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

    #[test]
    fn axis_aligned_hit_reports_entry_face() {
        let mut v = vol(8);
        v.set(IVec3::new(3, 2, 5), 1);
        v.set(IVec3::new(3, 2, 6), 1);
        let ray = Ray::new(Vec3::new(3.5, 2.5, -10.0), Vec3::Z);
        let hit = raycast_volume(&ray, &v).unwrap();
        assert_eq!(hit.voxel, IVec3::new(3, 2, 5));
        assert_eq!(hit.normal, IVec3::NEG_Z);
        assert_eq!(hit.adjacent(), IVec3::new(3, 2, 4));
        assert!((hit.t - 15.0).abs() < 1e-4);
        assert_eq!(hit.face_coord(), 5);

        let down = Ray::new(Vec3::new(3.5, 20.0, 5.5), Vec3::NEG_Y);
        let hit = raycast_volume(&down, &v).unwrap();
        assert_eq!((hit.voxel, hit.normal), (IVec3::new(3, 2, 5), IVec3::Y));
        assert_eq!(hit.face_coord(), 3);
    }

    #[test]
    fn diagonal_and_corner_exact_rays() {
        let mut v = vol(8);
        v.set(IVec3::new(4, 4, 4), 2);
        // Through the exact corner (4, 4, 4) of the target cell.
        let ray = Ray::new(Vec3::ZERO, Vec3::ONE);
        let hit = raycast_volume(&ray, &v).unwrap();
        assert_eq!(hit.voxel, IVec3::new(4, 4, 4));
        assert_eq!(hit.normal.abs().element_sum(), 1, "one face normal");
        // Grid walk through empty cells before the hit.
        let area = VoxelBox::from_bounds(v.bounds());
        let hit = raycast_grid(&ray, &area, |p| v.get(p) != 0).unwrap();
        assert_eq!(hit.voxel, IVec3::new(4, 4, 4));
    }

    #[test]
    fn misses_return_none() {
        let mut v = vol(8);
        v.set(IVec3::ZERO, 1);
        let away = Ray::new(Vec3::new(0.5, 0.5, -1.0), Vec3::NEG_Z);
        assert_eq!(raycast_volume(&away, &v), None);
        let past = Ray::new(Vec3::new(5.5, 0.5, -1.0), Vec3::Z);
        assert_eq!(raycast_volume(&past, &v), None);
        assert_eq!(raycast_volume(&past, &vol(8)), None, "empty volume");
        let zero = Ray {
            origin: Vec3::ZERO,
            dir: Vec3::ZERO,
        };
        assert_eq!(raycast_volume(&zero, &v), None);
    }

    #[test]
    fn ray_starting_inside_a_voxel() {
        let mut v = vol(8);
        v.set(IVec3::new(2, 2, 2), 1);
        let ray = Ray::new(Vec3::splat(2.5), Vec3::X);
        let hit = raycast_volume(&ray, &v).unwrap();
        assert_eq!(hit.voxel, IVec3::splat(2));
        assert_eq!(hit.t, 0.0);
        assert_eq!(hit.normal, IVec3::NEG_X);
    }

    #[test]
    fn hit_on_last_voxel_of_bounds() {
        let mut v = vol(8);
        v.set(IVec3::new(7, 7, 7), 3);
        let area = VoxelBox::from_bounds(v.bounds());
        let ray = Ray::new(Vec3::new(7.5, 7.5, -3.0), Vec3::Z);
        let hit = raycast_grid(&ray, &area, |p| v.get(p) != 0).unwrap();
        assert_eq!((hit.voxel, hit.normal), (IVec3::splat(7), IVec3::NEG_Z));
        // Coming from beyond the far face.
        let back = Ray::new(Vec3::new(7.5, 7.5, 30.0), Vec3::NEG_Z);
        let hit = raycast_grid(&back, &area, |p| v.get(p) != 0).unwrap();
        assert_eq!((hit.voxel, hit.normal), (IVec3::splat(7), IVec3::Z));
    }

    #[test]
    fn negative_coordinates() {
        let area = VoxelBox::from_corners(IVec3::splat(-8), IVec3::splat(-1));
        let target = IVec3::new(-3, -5, -2);
        let ray = Ray::new(Vec3::new(10.0, -4.5, -1.5), Vec3::NEG_X);
        let hit = raycast_grid(&ray, &area, |p| p == target).unwrap();
        assert_eq!((hit.voxel, hit.normal), (target, IVec3::X));
    }

    #[test]
    fn floor_pick_and_face_plane() {
        let b = BoundSize::new(8, 8, 8);
        let ray = Ray::new(Vec3::new(2.5, 10.0, 3.5), Vec3::NEG_Y);
        let hit = raycast_floor(&ray, b).unwrap();
        assert_eq!(hit.adjacent(), IVec3::new(2, 0, 3));
        let outside = Ray::new(Vec3::new(20.5, 10.0, 3.5), Vec3::NEG_Y);
        assert_eq!(raycast_floor(&outside, b), None);
        let p = raycast_face(&ray, 1, 4.0).unwrap();
        assert!((p - Vec3::new(2.5, 4.0, 3.5)).length() < 1e-5);
        assert_eq!(raycast_face(&ray, 0, 1.0), None, "parallel");
        // `pick` prefers voxels over the floor.
        let area = VoxelBox::from_corners(IVec3::new(2, 0, 3), IVec3::new(2, 0, 3));
        let hit = pick(&ray, b, Some(area), |_| true).unwrap();
        assert_eq!(hit.voxel, IVec3::new(2, 0, 3));
    }
}
