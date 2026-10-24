//! Core operations (Qubicle *Model ▸ Core*): fill the closed cavities of a
//! model, or hollow it out to a shell.
//!
//! Both work inside the voxels' box grown by one cell, on flat arrays, so a
//! full 256³ model stays fast; [`MAX_CORE_CELLS`] caps the box.

use std::collections::VecDeque;

use glam::IVec3;
use rizxel_math::{Voxel, VoxelVolume, EMPTY_VOXEL};

use super::{FloodCapped, VoxelChange, NEIGHBORS_6};

/// Largest box (cells, after growing by one) a core operation works on.
pub const MAX_CORE_CELLS: usize = 64 * 1024 * 1024;

/// Dense index over the box `lo..hi`.
struct Grid {
    lo: IVec3,
    size: IVec3,
}

impl Grid {
    /// Box of `volume`'s voxels grown by `pad`; `None` for an empty volume.
    fn around(volume: &VoxelVolume, pad: i32) -> Result<Option<Grid>, FloodCapped> {
        let Some((lo, hi)) = volume.occupied_bounds() else {
            return Ok(None);
        };
        let (lo, hi) = (lo - IVec3::splat(pad), hi + IVec3::splat(pad));
        let size = hi - lo;
        let cells = size.as_i64vec3().element_product();
        if cells > MAX_CORE_CELLS as i64 {
            return Err(FloodCapped {
                limit: MAX_CORE_CELLS,
            });
        }
        Ok(Some(Grid { lo, size }))
    }

    fn len(&self) -> usize {
        self.size.as_i64vec3().element_product() as usize
    }

    fn index(&self, p: IVec3) -> Option<usize> {
        let l = p - self.lo;
        (l.cmpge(IVec3::ZERO).all() && l.cmplt(self.size).all())
            .then(|| (l.x + self.size.x * (l.y + self.size.y * l.z)) as usize)
    }

    fn cell(&self, i: usize) -> IVec3 {
        let i = i as i32;
        let x = i % self.size.x;
        let y = (i / self.size.x) % self.size.y;
        let z = i / (self.size.x * self.size.y);
        self.lo + IVec3::new(x, y, z)
    }
}

/// *Fill Hollows*: every empty cell inside the bounds that the outside
/// cannot reach through empty cells (6-connected) gets `color`. Empty space
/// beyond the bounds counts as outside.
pub fn fill_hollows(volume: &VoxelVolume, color: Voxel) -> Result<Vec<VoxelChange>, FloodCapped> {
    let Some(grid) = Grid::around(volume, 1)? else {
        return Ok(Vec::new());
    };
    let solid = |p: IVec3| volume.get(p) != EMPTY_VOXEL;
    // Flood the outside from the grid's corner (always empty: it is padding).
    let mut outside = vec![false; grid.len()];
    let mut queue = VecDeque::from([grid.lo]);
    outside[0] = true;
    while let Some(p) = queue.pop_front() {
        for n in NEIGHBORS_6 {
            let q = p + n;
            // Cells outside the bounds are open air even inside the grid.
            if let Some(i) = grid.index(q) {
                if !outside[i] && (!volume.in_bounds(q) || !solid(q)) {
                    outside[i] = true;
                    queue.push_back(q);
                }
            }
        }
    }
    Ok((0..grid.len())
        .filter(|&i| !outside[i])
        .map(|i| grid.cell(i))
        .filter(|&p| volume.in_bounds(p) && !solid(p))
        .map(|pos| VoxelChange {
            pos,
            old: EMPTY_VOXEL,
            new: color,
        })
        .collect())
}

/// *Hollow*: removes every voxel whose six neighbours are all solid,
/// leaving a one-voxel shell (out-of-bounds neighbours count as empty).
pub fn hollow(volume: &VoxelVolume) -> Result<Vec<VoxelChange>, FloodCapped> {
    hollow_advanced(volume, 1)
}

/// *Advanced Hollow*: keeps a shell `thickness` voxels deep, measured in
/// face steps from the nearest exposed voxel (one with an empty face
/// neighbour, including the faces of inner cavities). `thickness` 0 is
/// treated as 1.
pub fn hollow_advanced(
    volume: &VoxelVolume,
    thickness: u32,
) -> Result<Vec<VoxelChange>, FloodCapped> {
    let Some(grid) = Grid::around(volume, 0)? else {
        return Ok(Vec::new());
    };
    let thickness = thickness.max(1);
    let solid = |p: IVec3| volume.get(p) != EMPTY_VOXEL;
    // Depth 1 = exposed voxels; breadth-first through solid voxels.
    let mut depth = vec![0u32; grid.len()];
    let mut queue = VecDeque::new();
    for (p, _) in volume.iter() {
        if NEIGHBORS_6.iter().any(|&n| !solid(p + n)) {
            if let Some(i) = grid.index(p) {
                depth[i] = 1;
                queue.push_back(p);
            }
        }
    }
    while let Some(p) = queue.pop_front() {
        let d = grid.index(p).map_or(0, |i| depth[i]);
        if d >= thickness {
            continue;
        }
        for n in NEIGHBORS_6 {
            let q = p + n;
            if let Some(i) = grid.index(q) {
                if depth[i] == 0 && solid(q) {
                    depth[i] = d + 1;
                    queue.push_back(q);
                }
            }
        }
    }
    Ok(volume
        .iter()
        .filter(|&(p, _)| grid.index(p).is_some_and(|i| depth[i] == 0))
        .map(|(pos, old)| VoxelChange {
            pos,
            old,
            new: EMPTY_VOXEL,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    fn solid_box(v: &mut VoxelVolume, lo: IVec3, hi: IVec3, c: Voxel) {
        for z in lo.z..hi.z {
            for y in lo.y..hi.y {
                for x in lo.x..hi.x {
                    v.set(IVec3::new(x, y, z), c);
                }
            }
        }
    }

    fn apply(v: &mut VoxelVolume, changes: &[VoxelChange]) {
        for c in changes {
            v.set(c.pos, c.new);
        }
    }

    #[test]
    fn hollow_of_a_solid_5_cube_leaves_98_voxels() {
        let mut v = VoxelVolume::new(BoundSize::new(16, 16, 16));
        solid_box(&mut v, IVec3::ONE, IVec3::splat(6), 3);
        let changes = hollow(&v).unwrap();
        assert_eq!(changes.len(), 27);
        apply(&mut v, &changes);
        assert_eq!(v.voxel_count(), 98);
        // Hollowing a shell again changes nothing.
        assert!(hollow(&v).unwrap().is_empty());
    }

    #[test]
    fn advanced_hollow_keeps_the_requested_thickness() {
        let mut v = VoxelVolume::new(BoundSize::new(16, 16, 16));
        solid_box(&mut v, IVec3::ZERO, IVec3::splat(7), 3);
        // Thickness 2 keeps the outer two layers: 7³ - 3³.
        let changes = hollow_advanced(&v, 2).unwrap();
        assert_eq!(changes.len(), 27);
        assert!(hollow_advanced(&v, 4).unwrap().is_empty());
        assert_eq!(hollow_advanced(&v, 0).unwrap(), hollow(&v).unwrap());
    }

    #[test]
    fn a_closed_shell_fills_exactly_its_interior() {
        let mut v = VoxelVolume::new(BoundSize::new(16, 16, 16));
        solid_box(&mut v, IVec3::new(2, 2, 2), IVec3::new(8, 7, 6), 3);
        let interior: usize = 4 * 3 * 2;
        let shell = hollow(&v).unwrap();
        assert_eq!(shell.len(), interior);
        apply(&mut v, &shell);
        let fill = fill_hollows(&v, 5).unwrap();
        assert_eq!(fill.len(), interior);
        assert!(fill.iter().all(|c| shell.iter().any(|s| s.pos == c.pos)));
        assert!(fill.iter().all(|c| c.old == 0 && c.new == 5));
        // An opening lets the outside in: nothing to fill.
        v.set(IVec3::new(2, 4, 3), EMPTY_VOXEL);
        assert!(fill_hollows(&v, 5).unwrap().is_empty());
    }

    #[test]
    fn a_cup_open_past_the_bounds_is_not_a_hollow() {
        // Walls touch the bounds on -Y: the cavity is open through the
        // bounds face, which counts as outside.
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        solid_box(&mut v, IVec3::ZERO, IVec3::new(5, 5, 5), 3);
        solid_box(&mut v, IVec3::new(1, 0, 1), IVec3::new(4, 4, 4), 0);
        assert!(fill_hollows(&v, 5).unwrap().is_empty());
        assert!(fill_hollows(&VoxelVolume::new(BoundSize::new(4, 4, 4)), 5)
            .unwrap()
            .is_empty());
    }
}
