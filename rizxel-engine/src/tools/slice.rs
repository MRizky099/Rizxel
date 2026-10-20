//! Plane tool, Slice mode: cuts the model along an axis-aligned plane,
//! clears one half and optionally caps the opening.
//!
//! The cut always lies on a voxel boundary: `KeepSide::Positive` keeps
//! voxels with `coord >= cut`, `KeepSide::Negative` keeps `coord < cut`.
//! The cap is a one-voxel lid in the current color on the first discarded
//! layer, covering the model's cross-section there with its enclosed holes
//! filled, so the cut face is closed and flat.

use std::collections::{HashMap, VecDeque};

use glam::IVec3;
use rizxel_math::{BoundSize, Voxel, VoxelVolume, EMPTY_VOXEL};

use super::{filter_placeable, KeepSide, VoxelChange};

/// A resolved cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceRequest {
    /// `0 = X`, `1 = Y`, `2 = Z`.
    pub axis: usize,
    /// Boundary coordinate of the cut plane, in `0..=bounds[axis]`.
    pub cut: i32,
    pub keep: KeepSide,
    pub cap: bool,
}

impl SliceRequest {
    /// Coordinate of the first discarded layer (where the cap goes).
    pub fn discarded_layer(&self) -> i32 {
        match self.keep {
            KeepSide::Positive => self.cut - 1,
            KeepSide::Negative => self.cut,
        }
    }
}

/// Changes that slice one volume. Only occupied tiles are visited, so the
/// cost follows the voxel count, not the bounds volume.
pub fn slice_volume(
    volume: &VoxelVolume,
    req: &SliceRequest,
    cap_color: Voxel,
) -> Vec<VoxelChange> {
    let axis = req.axis;
    let lid = req.discarded_layer();
    let mut changes = Vec::new();
    let mut section = Vec::new();
    for (pos, old) in volume.iter() {
        if req.keep.keeps(pos[axis], req.cut) {
            continue;
        }
        changes.push(VoxelChange {
            pos,
            old,
            new: EMPTY_VOXEL,
        });
        if pos[axis] == lid {
            section.push(pos);
        }
    }
    if req.cap && cap_color != EMPTY_VOXEL && !section.is_empty() {
        let index: HashMap<IVec3, usize> = changes
            .iter()
            .enumerate()
            .map(|(i, c)| (c.pos, i))
            .collect();
        for pos in close_section(&section, axis) {
            match index.get(&pos) {
                Some(&i) => changes[i].new = cap_color,
                None => changes.push(VoxelChange {
                    pos,
                    old: volume.get(pos),
                    new: cap_color,
                }),
            }
        }
    }
    filter_placeable(changes, volume.bounds())
}

/// `section` (cells on one plane) plus every empty cell it encloses: a 2D
/// flood fill from a ring around the section's bounding rectangle marks the
/// outside; whatever is neither section nor outside is a hole.
fn close_section(section: &[IVec3], axis: usize) -> Vec<IVec3> {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let lo = section.iter().fold(IVec3::MAX, |a, p| a.min(*p)) - IVec3::ONE;
    let hi = section.iter().fold(IVec3::MIN, |a, p| a.max(*p)) + IVec3::ONE;
    let (w, h) = ((hi[u] - lo[u] + 1) as usize, (hi[v] - lo[v] + 1) as usize);
    let idx = |i: usize, j: usize| j * w + i;
    // 0 = unknown, 1 = section, 2 = outside.
    let mut grid = vec![0u8; w * h];
    for p in section {
        grid[idx((p[u] - lo[u]) as usize, (p[v] - lo[v]) as usize)] = 1;
    }
    let mut queue = VecDeque::from([(0usize, 0usize)]);
    grid[0] = 2;
    while let Some((i, j)) = queue.pop_front() {
        let neighbours = [
            (i.wrapping_sub(1), j),
            (i + 1, j),
            (i, j.wrapping_sub(1)),
            (i, j + 1),
        ];
        for (ni, nj) in neighbours {
            if ni < w && nj < h && grid[idx(ni, nj)] == 0 {
                grid[idx(ni, nj)] = 2;
                queue.push_back((ni, nj));
            }
        }
    }
    let plane = section[0][axis];
    let mut out = Vec::new();
    for j in 0..h {
        for i in 0..w {
            if grid[idx(i, j)] != 2 {
                let mut p = IVec3::ZERO;
                p[axis] = plane;
                p[u] = lo[u] + i as i32;
                p[v] = lo[v] + j as i32;
                out.push(p);
            }
        }
    }
    out
}

/// Slices every given layer (the caller passes the visible, unlocked voxel
/// layers). Layers without changes are left out, so the undo entry stays
/// minimal.
pub fn slice_layers(
    volumes: &[(usize, &VoxelVolume)],
    bounds: BoundSize,
    req: &SliceRequest,
    cap_color: Voxel,
) -> Vec<(usize, Vec<VoxelChange>)> {
    volumes
        .iter()
        .filter_map(|&(layer, volume)| {
            let changes = filter_placeable(slice_volume(volume, req, cap_color), bounds);
            (!changes.is_empty()).then_some((layer, changes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(bounds: BoundSize, size: IVec3) -> VoxelVolume {
        let mut v = VoxelVolume::new(bounds);
        for z in 0..size.z {
            for y in 0..size.y {
                for x in 0..size.x {
                    v.set(IVec3::new(x, y, z), 1);
                }
            }
        }
        v
    }

    fn applied(v: &VoxelVolume, changes: &[VoxelChange]) -> VoxelVolume {
        let mut out = v.clone();
        for c in changes {
            out.set(c.pos, c.new);
        }
        out
    }

    fn req(axis: usize, cut: i32, keep: KeepSide, cap: bool) -> SliceRequest {
        SliceRequest {
            axis,
            cut,
            keep,
            cap,
        }
    }

    #[test]
    fn cut_keeps_each_half() {
        let b = BoundSize::new(4, 4, 4);
        let v = solid(b, IVec3::splat(4));
        let pos = applied(
            &v,
            &slice_volume(&v, &req(0, 2, KeepSide::Positive, false), 5),
        );
        assert_eq!(pos.voxel_count(), 32);
        assert!(pos.iter().all(|(p, _)| p.x >= 2));
        let neg = applied(
            &v,
            &slice_volume(&v, &req(0, 2, KeepSide::Negative, false), 5),
        );
        assert_eq!(neg.voxel_count(), 32);
        assert!(neg.iter().all(|(p, _)| p.x < 2));
    }

    #[test]
    fn every_axis_gives_symmetric_results() {
        let b = BoundSize::new(8, 8, 8);
        for axis in 0..3 {
            let mut size = IVec3::new(4, 6, 2);
            // Rotate the box so the cut axis always spans 4 voxels.
            size = IVec3::from_array(std::array::from_fn(|i| size[(i + 3 - axis) % 3]));
            let v = solid(b, size);
            let kept = applied(
                &v,
                &slice_volume(&v, &req(axis, 1, KeepSide::Positive, false), 1),
            );
            assert_eq!(kept.voxel_count(), 3 * 6 * 2, "axis {axis}");
        }
    }

    #[test]
    fn cap_closes_the_cut_face() {
        let b = BoundSize::new(4, 4, 4);
        let v = solid(b, IVec3::splat(4));
        let plain = applied(
            &v,
            &slice_volume(&v, &req(1, 2, KeepSide::Positive, false), 9),
        );
        let capped = applied(
            &v,
            &slice_volume(&v, &req(1, 2, KeepSide::Positive, true), 9),
        );
        assert_eq!(capped.voxel_count() - plain.voxel_count(), 4 * 4);
        for z in 0..4 {
            for x in 0..4 {
                assert_eq!(capped.get(IVec3::new(x, 1, z)), 9);
            }
        }
    }

    #[test]
    fn cap_fills_enclosed_holes() {
        // Hollow 5x5 tube along Y: the cap must close its 3x3 inside.
        let b = BoundSize::new(5, 5, 5);
        let mut v = solid(b, IVec3::splat(5));
        for y in 0..5 {
            for z in 1..4 {
                for x in 1..4 {
                    v.clear_at(IVec3::new(x, y, z));
                }
            }
        }
        let r = req(1, 3, KeepSide::Negative, true);
        let capped = applied(&v, &slice_volume(&v, &r, 7));
        for z in 0..5 {
            for x in 0..5 {
                assert_eq!(capped.get(IVec3::new(x, 3, z)), 7, "({x}, {z})");
            }
        }
        assert_eq!(capped.get(IVec3::new(2, 4, 2)), 0);
    }

    #[test]
    fn empty_side_produces_no_changes() {
        let b = BoundSize::new(8, 8, 8);
        let v = solid(b, IVec3::new(8, 2, 8));
        let r = req(1, 4, KeepSide::Negative, true);
        assert!(slice_volume(&v, &r, 3).is_empty());
        assert!(slice_layers(&[(0, &v)], b, &r, 3).is_empty());
        let r = req(1, 1, KeepSide::Negative, false);
        let out = slice_layers(&[(0, &v), (1, &VoxelVolume::new(b))], b, &r, 3);
        assert_eq!(out.len(), 1, "empty layers are skipped");
        assert_eq!(out[0].1.len(), 64);
    }
}
