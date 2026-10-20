//! Color picker: reads the palette index under the cursor.

use glam::IVec3;
use rizxel_math::{Voxel, VoxelVolume, EMPTY_VOXEL};

/// Palette index at `pos` in the topmost volume (last in `volumes`) that has
/// a voxel there, or `None` when every volume is empty at `pos`.
pub fn pick_voxel(volumes: &[&VoxelVolume], pos: IVec3) -> Option<Voxel> {
    volumes
        .iter()
        .rev()
        .map(|v| v.get(pos))
        .find(|&v| v != EMPTY_VOXEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    #[test]
    fn topmost_layer_wins() {
        let b = BoundSize::new(4, 4, 4);
        let mut low = VoxelVolume::new(b);
        let mut high = VoxelVolume::new(b);
        low.set(IVec3::ZERO, 3);
        low.set(IVec3::X, 4);
        high.set(IVec3::ZERO, 9);
        assert_eq!(pick_voxel(&[&low, &high], IVec3::ZERO), Some(9));
        assert_eq!(pick_voxel(&[&low, &high], IVec3::X), Some(4));
        assert_eq!(pick_voxel(&[&low, &high], IVec3::Y), None);
    }
}
