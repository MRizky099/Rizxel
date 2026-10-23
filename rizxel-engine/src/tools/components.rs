//! Connected parts of a layer (Qubicle *Separate Parts*).

use std::collections::HashSet;

use glam::IVec3;
use rizxel_math::{VoxelVolume, EMPTY_VOXEL};

use super::{flood, FloodCapped, MAX_FLOOD_CELLS};

/// The 6-connected groups of solid voxels of `volume`, largest first (ties
/// keep the order of their first voxel). Refused, like any flood, past
/// [`MAX_FLOOD_CELLS`] voxels.
pub fn connected_components(volume: &VoxelVolume) -> Result<Vec<Vec<IVec3>>, FloodCapped> {
    if volume.voxel_count() > MAX_FLOOD_CELLS {
        return Err(FloodCapped {
            limit: MAX_FLOOD_CELLS,
        });
    }
    let mut seen: HashSet<IVec3> = HashSet::new();
    let mut parts = Vec::new();
    for (seed, _) in volume.iter() {
        if seen.contains(&seed) {
            continue;
        }
        let part = flood(seed, MAX_FLOOD_CELLS, |p| {
            !seen.contains(&p) && volume.get(p) != EMPTY_VOXEL
        })?;
        seen.extend(part.iter().copied());
        parts.push(part);
    }
    parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    #[test]
    fn two_separate_cubes_are_two_parts() {
        let mut v = VoxelVolume::new(BoundSize::new(16, 16, 16));
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    v.set(IVec3::new(x, y, z), 1);
                    v.set(IVec3::new(x + 5, y, z), 2);
                    v.set(IVec3::new(x + 8, y, z), 2);
                }
            }
        }
        // The second and third cubes touch through a bridge.
        v.set(IVec3::new(7, 0, 0), 3);
        let parts = connected_components(&v).unwrap();
        assert_eq!(parts.iter().map(Vec::len).collect::<Vec<_>>(), [17, 8]);
        // Diagonal neighbours are separate parts.
        let mut d = VoxelVolume::new(BoundSize::new(4, 4, 4));
        d.set(IVec3::ZERO, 1);
        d.set(IVec3::ONE, 1);
        assert_eq!(connected_components(&d).unwrap().len(), 2);
        assert!(
            connected_components(&VoxelVolume::new(BoundSize::new(4, 4, 4)))
                .unwrap()
                .is_empty()
        );
    }
}
