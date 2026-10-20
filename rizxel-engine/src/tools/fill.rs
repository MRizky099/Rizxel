//! Fill: paint-bucket recolor of connected same-colored voxels, or a flood
//! of the empty space in front of the clicked face.
//!
//! Both floods go through [`super::flood`] and stop at
//! [`MAX_FLOOD_CELLS`]; a capped fill changes nothing.

use std::collections::HashSet;

use glam::IVec3;
use rizxel_math::{Palette, VoxelBox, VoxelVolume, EMPTY_VOXEL};

use super::{colors_match, flood, FillMode, FillOptions, FloodCapped, MAX_FLOOD_CELLS};
use crate::raycast::RayHit;

/// Why a fill produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillError {
    /// The region exceeded the cell cap.
    Capped(FloodCapped),
    /// The clicked cell cannot seed this fill (empty voxel for Recolor,
    /// solid or outside the model box for Fill empty, outside the selection).
    NothingToFill,
}

impl FillError {
    pub fn message(&self) -> String {
        match self {
            FillError::Capped(c) => c.message(),
            FillError::NothingToFill => "Nothing to fill here".to_owned(),
        }
    }
}

impl From<FloodCapped> for FillError {
    fn from(c: FloodCapped) -> Self {
        FillError::Capped(c)
    }
}

/// Cells a fill click at `hit` writes, found in `volume` (the region
/// source). `limit` confines the flood (bounds or the slice view) and
/// `allowed`, when given, restricts it to the selection.
///
/// - [`FillMode::Recolor`]: from `hit.voxel` over solid voxels whose color
///   is within `threshold` of the seed's (6-connected, or anywhere in the
///   volume when `connected` is off).
/// - [`FillMode::Empty`]: from `hit.adjacent()` over empty cells inside the
///   box around the model's voxels, so closed rooms and the inside of open
///   shells (cups) fill without flooding the whole bounds.
pub fn fill_cells(
    volume: &VoxelVolume,
    hit: &RayHit,
    palette: &Palette,
    opts: &FillOptions,
    limit: &VoxelBox,
    allowed: Option<&HashSet<IVec3>>,
) -> Result<Vec<IVec3>, FillError> {
    let permitted = |p: IVec3| limit.contains(p) && allowed.is_none_or(|s| s.contains(&p));
    let cells = match opts.mode {
        FillMode::Recolor => {
            let seed = hit.voxel;
            let seed_color = volume.get(seed);
            if seed_color == EMPTY_VOXEL || !permitted(seed) {
                return Err(FillError::NothingToFill);
            }
            let matches = |p: IVec3| {
                let v = volume.get(p);
                v != EMPTY_VOXEL
                    && permitted(p)
                    && colors_match(palette, v, seed_color, opts.threshold)
            };
            if opts.connected {
                flood(seed, MAX_FLOOD_CELLS, matches)?
            } else {
                let all: Vec<IVec3> = volume
                    .iter()
                    .map(|(p, _)| p)
                    .filter(|&p| matches(p))
                    .collect();
                if all.len() > MAX_FLOOD_CELLS {
                    return Err(FloodCapped {
                        limit: MAX_FLOOD_CELLS,
                    }
                    .into());
                }
                all
            }
        }
        FillMode::Empty => {
            let seed = hit.adjacent();
            let (lo, hi) = volume.occupied_bounds().ok_or(FillError::NothingToFill)?;
            let shell = VoxelBox { min: lo, max: hi };
            let empty =
                |p: IVec3| shell.contains(p) && permitted(p) && volume.get(p) == EMPTY_VOXEL;
            if !empty(seed) {
                return Err(FillError::NothingToFill);
            }
            flood(seed, MAX_FLOOD_CELLS, empty)?
        }
    };
    if cells.is_empty() {
        return Err(FillError::NothingToFill);
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    fn b() -> BoundSize {
        BoundSize::new(16, 16, 16)
    }

    /// Hollow 5³ box (walls one voxel thick) at the origin, color 1, with
    /// the lid (top face inside the rim) removed when `open`.
    fn room(open: bool) -> VoxelVolume {
        let mut v = VoxelVolume::new(b());
        for z in 0..5 {
            for y in 0..5 {
                for x in 0..5 {
                    let p = IVec3::new(x, y, z);
                    let wall = p.min_element() == 0 || p.max_element() == 4;
                    let lid = y == 4 && (1..4).contains(&x) && (1..4).contains(&z);
                    if wall && !(open && lid) {
                        v.set(p, 1);
                    }
                }
            }
        }
        v
    }

    fn floor_hit() -> RayHit {
        // Inner face of the floor, looking down at it from inside.
        RayHit {
            voxel: IVec3::new(2, 0, 2),
            normal: IVec3::Y,
            t: 0.0,
        }
    }

    fn opts(mode: FillMode) -> FillOptions {
        FillOptions {
            mode,
            ..FillOptions::default()
        }
    }

    #[test]
    fn closed_room_fills_its_inside() {
        let v = room(false);
        let all = VoxelBox::from_bounds(b());
        let p = Palette::default();
        let cells = fill_cells(&v, &floor_hit(), &p, &opts(FillMode::Empty), &all, None).unwrap();
        assert_eq!(cells.len(), 27);
        assert!(cells.iter().all(|&c| v.get(c) == EMPTY_VOXEL));
    }

    #[test]
    fn open_shell_fills_up_to_its_rim_only() {
        let v = room(true);
        let all = VoxelBox::from_bounds(b());
        let p = Palette::default();
        let cells = fill_cells(&v, &floor_hit(), &p, &opts(FillMode::Empty), &all, None).unwrap();
        // 3x3 inside columns from y = 1 to the rim layer y = 4.
        assert_eq!(cells.len(), 36);
        // Outside the model box there is nothing to fill.
        let outside = RayHit {
            voxel: IVec3::new(4, 2, 2),
            normal: IVec3::X,
            t: 0.0,
        };
        assert_eq!(
            fill_cells(&v, &outside, &p, &opts(FillMode::Empty), &all, None),
            Err(FillError::NothingToFill)
        );
    }

    #[test]
    fn recolor_follows_color_connectivity_and_selection() {
        let mut v = room(false);
        v.set(IVec3::new(2, 0, 2), 3);
        let all = VoxelBox::from_bounds(b());
        let p = Palette::default();
        let o = opts(FillMode::Recolor);
        let walls = fill_cells(&v, &floor_hit(), &p, &o, &all, None).unwrap();
        assert_eq!(walls, vec![IVec3::new(2, 0, 2)], "a lone color-3 voxel");
        let corner = RayHit {
            voxel: IVec3::ZERO,
            ..floor_hit()
        };
        // 5³ - 3³ wall voxels minus the recolored one.
        assert_eq!(
            fill_cells(&v, &corner, &p, &o, &all, None).unwrap().len(),
            97
        );
        let sel: HashSet<IVec3> = [IVec3::ZERO, IVec3::X, IVec3::splat(9)].into();
        assert_eq!(
            fill_cells(&v, &corner, &p, &o, &all, Some(&sel))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            fill_cells(&v, &floor_hit(), &p, &o, &all, Some(&sel)),
            Err(FillError::NothingToFill)
        );
    }

    #[test]
    fn fill_respects_the_limit_box() {
        let v = room(false);
        let p = Palette::default();
        let half = VoxelBox {
            min: IVec3::ZERO,
            max: IVec3::new(16, 16, 3),
        };
        let cells = fill_cells(&v, &floor_hit(), &p, &opts(FillMode::Empty), &half, None).unwrap();
        assert_eq!(cells.len(), 18, "inside z = 1..3 only");
    }
}
