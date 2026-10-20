//! Selection tool: box selection between two corner cells.
//!
//! The tool picks the voxel under the press as one corner and the voxel
//! (or the press face's plane) under the pointer as the other; every solid
//! cell inside the box joins the selection, combined with the existing one
//! by the [`super::MaskMode`]. The selection type itself lives in the
//! application layer; this module only computes cells.

use std::collections::HashSet;

use glam::IVec3;
use rizxel_math::{VoxelBox, VoxelVolume};

/// Box spanning the cells `a` and `b` (both included).
pub fn selection_box(a: IVec3, b: IVec3) -> VoxelBox {
    VoxelBox::from_corners(a, b)
}

/// Solid cells of any of `volumes` inside `area`, sorted (z, y, x) so the
/// result is deterministic.
pub fn select_box(volumes: &[&VoxelVolume], area: &VoxelBox) -> Vec<IVec3> {
    if !area.is_valid() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    for v in volumes {
        seen.extend(v.iter().map(|(p, _)| p).filter(|p| area.contains(*p)));
    }
    let mut out: Vec<IVec3> = seen.into_iter().collect();
    out.sort_by_key(|p| (p.z, p.y, p.x));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    #[test]
    fn box_selects_solid_cells_of_every_volume() {
        let b = BoundSize::new(8, 8, 8);
        let mut low = VoxelVolume::new(b);
        let mut high = VoxelVolume::new(b);
        for x in 0..4 {
            low.set(IVec3::new(x, 0, 0), 1);
        }
        high.set(IVec3::new(1, 0, 0), 2);
        high.set(IVec3::new(1, 1, 0), 2);
        let area = selection_box(IVec3::new(2, 1, 0), IVec3::ZERO);
        assert_eq!((area.min, area.max), (IVec3::ZERO, IVec3::new(3, 2, 1)));
        assert_eq!(select_box(&[&low], &area).len(), 3);
        // Overlapping voxels count once.
        assert_eq!(
            select_box(&[&low, &high], &area),
            vec![
                IVec3::new(0, 0, 0),
                IVec3::new(1, 0, 0),
                IVec3::new(2, 0, 0),
                IVec3::new(1, 1, 0),
            ]
        );
        // A single-cell box on an empty cell selects nothing.
        let empty = selection_box(IVec3::splat(5), IVec3::splat(5));
        assert!(select_box(&[&low, &high], &empty).is_empty());
    }
}
