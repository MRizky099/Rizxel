//! Extrude: pushes or pulls the face region under the cursor along its
//! normal. Without a selection (Phase 5b) the region is the contiguous run
//! of exposed faces around the hit voxel on the hit face's plane.

use std::collections::{HashSet, VecDeque};

use glam::IVec3;
use rizxel_math::{Palette, VoxelVolume, EMPTY_VOXEL};

use super::{filter_placeable, ExtrudeOptions, VoxelChange};
use crate::raycast::RayHit;

/// Upper bound on the region size (one full 256² bounds face).
const MAX_REGION: usize = 256 * 256;

/// Voxels whose face toward `hit.normal` is exposed and that connect to the
/// hit voxel through 4-neighbours on the face plane. With
/// [`ExtrudeOptions::use_color`] only voxels whose color differs by at most
/// `threshold` per channel join the region.
pub fn extrude_region(
    volume: &VoxelVolume,
    hit: &RayHit,
    opts: &ExtrudeOptions,
    palette: &Palette,
) -> Vec<IVec3> {
    let n = hit.normal;
    let start_color = volume.get(hit.voxel);
    if start_color == EMPTY_VOXEL || n.abs().element_sum() != 1 {
        return Vec::new();
    }
    let rgb = |v| palette.get(v as usize).unwrap_or_default();
    let base = rgb(start_color);
    let matches = |p: IVec3| {
        let v = volume.get(p);
        if v == EMPTY_VOXEL || volume.get(p + n) != EMPTY_VOXEL {
            return false;
        }
        if !opts.use_color || v == start_color {
            return true;
        }
        let c = rgb(v);
        let diff = [
            c.r.abs_diff(base.r),
            c.g.abs_diff(base.g),
            c.b.abs_diff(base.b),
        ];
        diff.into_iter().max().unwrap_or(0) <= opts.threshold
    };
    if !matches(hit.voxel) {
        return Vec::new();
    }
    let axis = hit.axis();
    let steps: Vec<IVec3> = (0..3)
        .filter(|&a| a != axis)
        .flat_map(|a| {
            let mut d = IVec3::ZERO;
            d[a] = 1;
            [d, -d]
        })
        .collect();
    let mut seen = HashSet::from([hit.voxel]);
    let mut queue = VecDeque::from([hit.voxel]);
    let mut out = Vec::new();
    while let Some(p) = queue.pop_front() {
        out.push(p);
        if out.len() >= MAX_REGION {
            break;
        }
        for &d in &steps {
            let q = p + d;
            if !seen.contains(&q) && matches(q) {
                seen.insert(q);
                queue.push_back(q);
            }
        }
    }
    out
}

/// Changes extruding `region` of `target` by `depth` voxels along `normal`.
/// Positive depths build outward, copying each face voxel's color from
/// `colors` (the volume the region was found in); negative depths carve
/// inward, starting with the face voxels themselves.
pub fn extrude_changes(
    target: &VoxelVolume,
    colors: &VoxelVolume,
    region: &[IVec3],
    normal: IVec3,
    depth: i32,
) -> Vec<VoxelChange> {
    let mut changes = Vec::new();
    for &p in region {
        if depth > 0 {
            let color = colors.get(p);
            for k in 1..=depth {
                let pos = p + normal * k;
                changes.push(VoxelChange {
                    pos,
                    old: target.get(pos),
                    new: color,
                });
            }
        } else {
            for k in 0..-depth {
                let pos = p - normal * k;
                changes.push(VoxelChange {
                    pos,
                    old: target.get(pos),
                    new: EMPTY_VOXEL,
                });
            }
        }
    }
    filter_placeable(changes, target.bounds())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    /// 3x1x3 slab at y = 0 with one differently colored corner.
    fn slab() -> VoxelVolume {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        for z in 0..3 {
            for x in 0..3 {
                v.set(IVec3::new(x, 0, z), 1);
            }
        }
        v.set(IVec3::new(2, 0, 2), 6);
        v
    }

    fn top_hit() -> RayHit {
        RayHit {
            voxel: IVec3::new(1, 0, 1),
            normal: IVec3::Y,
            t: 0.0,
        }
    }

    #[test]
    fn region_follows_exposed_faces_and_color() {
        let v = slab();
        let palette = Palette::default();
        let mut opts = ExtrudeOptions::default();
        assert_eq!(extrude_region(&v, &top_hit(), &opts, &palette).len(), 9);
        opts.use_color = true;
        assert_eq!(extrude_region(&v, &top_hit(), &opts, &palette).len(), 8);
        // Covered faces are not part of the region.
        let mut covered = slab();
        covered.set(IVec3::new(0, 1, 0), 1);
        opts.use_color = false;
        assert_eq!(
            extrude_region(&covered, &top_hit(), &opts, &palette).len(),
            8
        );
        let miss = RayHit {
            voxel: IVec3::new(5, 5, 5),
            ..top_hit()
        };
        assert!(extrude_region(&v, &miss, &opts, &palette).is_empty());
    }

    #[test]
    fn extrude_depth_out_and_in() {
        let v = slab();
        let region = extrude_region(
            &v,
            &top_hit(),
            &ExtrudeOptions::default(),
            &Palette::default(),
        );
        let up = extrude_changes(&v, &v, &region, IVec3::Y, 3);
        assert_eq!(up.len(), 27);
        assert!(up
            .iter()
            .any(|c| c.pos == IVec3::new(2, 3, 2) && c.new == 6));
        let down = extrude_changes(&v, &v, &region, IVec3::Y, -1);
        assert_eq!(down.len(), 9);
        assert!(down.iter().all(|c| c.new == EMPTY_VOXEL && c.pos.y == 0));
        // Beyond the bounds nothing is written.
        assert_eq!(extrude_changes(&v, &v, &region, IVec3::Y, 20).len(), 9 * 7);
        assert!(extrude_changes(&v, &v, &region, IVec3::Y, 0).is_empty());
    }
}
