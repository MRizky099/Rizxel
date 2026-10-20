//! Whole-volume greedy quads for file exports.
//!
//! Unlike the viewport mesher (which merges per 16³ tile so edits can re-mesh
//! incrementally), exports merge across the whole model so files carry as
//! few faces as possible. Quads are produced in a fixed order (axis, then
//! negative before positive, then slice, then `v`, then `u`) so output files
//! are deterministic.

use glam::IVec3;
use rizxel_math::{Voxel, VoxelVolume, EMPTY_VOXEL};

/// One merged face rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quad {
    /// Axis the face points along (0 = X, 1 = Y, 2 = Z).
    pub axis: usize,
    /// Face points to `+axis` when true, `-axis` otherwise.
    pub positive: bool,
    /// Corner coordinate of the face plane along `axis`.
    pub plane: i32,
    /// Corner range along the first in-plane axis (`min(other axes)`).
    pub u: (i32, i32),
    /// Corner range along the second in-plane axis (`max(other axes)`).
    pub v: (i32, i32),
    pub value: Voxel,
}

impl Quad {
    /// In-plane axes `(u, v)`, ascending.
    pub fn uv_axes(axis: usize) -> (usize, usize) {
        match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        }
    }

    /// Corner points `[A, B, C, D]` = `(u0,v0) (u1,v0) (u0,v1) (u1,v1)`.
    pub fn corners(&self) -> [IVec3; 4] {
        let (ua, va) = Self::uv_axes(self.axis);
        let at = |u: i32, v: i32| {
            let mut p = IVec3::ZERO;
            p[self.axis] = self.plane;
            p[ua] = u;
            p[va] = v;
            p
        };
        [
            at(self.u.0, self.v.0),
            at(self.u.1, self.v.0),
            at(self.u.0, self.v.1),
            at(self.u.1, self.v.1),
        ]
    }

    /// True when `A, B, C` winds counter-clockwise seen from the face side
    /// (`u × v` points along the face normal).
    pub fn abc_is_ccw(&self) -> bool {
        // u × v is +X for (Y,Z), -Y for (X,Z) and +Z for (X,Y).
        let uv_sign = if self.axis == 1 { -1 } else { 1 };
        (uv_sign > 0) == self.positive
    }
}

/// Dense copy of the occupied part of a volume, for fast neighbour reads.
struct Dense {
    min: IVec3,
    size: IVec3,
    cells: Vec<Voxel>,
}

impl Dense {
    fn new(volume: &VoxelVolume) -> Option<Self> {
        let (min, max) = volume.occupied_bounds()?;
        let size = max - min;
        let mut cells = vec![EMPTY_VOXEL; (size.x * size.y * size.z) as usize];
        for (p, v) in volume.iter() {
            let l = p - min;
            cells[((l.z * size.y + l.y) * size.x + l.x) as usize] = v;
        }
        Some(Self { min, size, cells })
    }

    fn get(&self, p: IVec3) -> Voxel {
        let l = p - self.min;
        if l.cmplt(IVec3::ZERO).any() || l.cmpge(self.size).any() {
            return EMPTY_VOXEL;
        }
        self.cells[((l.z * self.size.y + l.y) * self.size.x + l.x) as usize]
    }
}

/// Greedy quads of every exposed face, merged per slice across the whole
/// volume (same palette index only). Faces at the bounds are exposed.
pub fn greedy_quads(volume: &VoxelVolume) -> Vec<Quad> {
    let Some(dense) = Dense::new(volume) else {
        return Vec::new();
    };
    let (min, max) = (dense.min, dense.min + dense.size);
    let mut quads = Vec::new();
    for axis in 0..3 {
        let (ua, va) = Quad::uv_axes(axis);
        let (nu, nv) = (dense.size[ua] as usize, dense.size[va] as usize);
        let mut mask = vec![EMPTY_VOXEL; nu * nv];
        for positive in [false, true] {
            let mut step = IVec3::ZERO;
            step[axis] = if positive { 1 } else { -1 };
            for s in min[axis]..max[axis] {
                let mut any = false;
                for v in 0..nv {
                    for u in 0..nu {
                        let mut p = IVec3::ZERO;
                        p[axis] = s;
                        p[ua] = min[ua] + u as i32;
                        p[va] = min[va] + v as i32;
                        let value = dense.get(p);
                        let face = value != EMPTY_VOXEL && dense.get(p + step) == EMPTY_VOXEL;
                        mask[v * nu + u] = if face { value } else { EMPTY_VOXEL };
                        any |= face;
                    }
                }
                if !any {
                    continue;
                }
                let plane = if positive { s + 1 } else { s };
                for v in 0..nv {
                    let mut u = 0;
                    while u < nu {
                        let value = mask[v * nu + u];
                        if value == EMPTY_VOXEL {
                            u += 1;
                            continue;
                        }
                        let mut w = 1;
                        while u + w < nu && mask[v * nu + u + w] == value {
                            w += 1;
                        }
                        let mut h = 1;
                        while v + h < nv
                            && mask[(v + h) * nu + u..(v + h) * nu + u + w]
                                .iter()
                                .all(|&m| m == value)
                        {
                            h += 1;
                        }
                        for row in v..v + h {
                            mask[row * nu + u..row * nu + u + w].fill(EMPTY_VOXEL);
                        }
                        let (u0, v0) = (min[ua] + u as i32, min[va] + v as i32);
                        quads.push(Quad {
                            axis,
                            positive,
                            plane,
                            u: (u0, u0 + w as i32),
                            v: (v0, v0 + h as i32),
                            value,
                        });
                        u += w;
                    }
                }
            }
        }
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    fn volume(cells: &[(IVec3, Voxel)]) -> VoxelVolume {
        let mut v = VoxelVolume::new(BoundSize::new(64, 64, 64));
        for &(p, c) in cells {
            v.set(p, c);
        }
        v
    }

    #[test]
    fn single_voxel_has_six_unit_quads() {
        let q = greedy_quads(&volume(&[(IVec3::new(3, 4, 5), 2)]));
        assert_eq!(q.len(), 6);
        assert!(q.iter().all(|q| q.u.1 - q.u.0 == 1 && q.v.1 - q.v.0 == 1));
    }

    #[test]
    fn merges_across_tile_borders() {
        // 40 voxels in a row cross two tile borders; the viewport mesher
        // splits them, the export quads do not.
        let row: Vec<_> = (0..40).map(|x| (IVec3::new(x, 0, 0), 7)).collect();
        assert_eq!(greedy_quads(&volume(&row)).len(), 6);
    }

    #[test]
    fn corners_wind_counter_clockwise_from_outside() {
        let q = greedy_quads(&volume(&[(IVec3::ZERO, 1)]));
        for quad in q {
            let [a, b, c, _] = quad.corners().map(|p| p.as_vec3());
            let (a, b, c) = if quad.abc_is_ccw() {
                (a, b, c)
            } else {
                (c, b, a)
            };
            let mut n = glam::Vec3::ZERO;
            n[quad.axis] = if quad.positive { 1.0 } else { -1.0 };
            assert!((b - a).cross(c - a).dot(n) > 0.0, "{quad:?}");
        }
    }

    #[test]
    fn empty_volume_has_no_quads() {
        assert!(greedy_quads(&volume(&[])).is_empty());
    }
}
