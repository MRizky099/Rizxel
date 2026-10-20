//! Fuzzy Select (magic wand): voxels connected to the clicked one whose
//! color is close enough to it.

use glam::IVec3;
use rizxel_math::{Palette, VoxelVolume, EMPTY_VOXEL};

use super::{colors_match, flood, FloodCapped, FuzzySelectOptions, MAX_FLOOD_CELLS};

/// Cells selected by a click on `seed`: the 6-connected solid voxels whose
/// palette color differs from the seed's by at most `threshold` per RGBA
/// channel (any solid voxel when `use_color` is off). With `entire_layer`
/// every matching voxel of `volume` is selected, connected or not.
pub fn fuzzy_select(
    volume: &VoxelVolume,
    seed: IVec3,
    palette: &Palette,
    opts: &FuzzySelectOptions,
) -> Result<Vec<IVec3>, FloodCapped> {
    let seed_color = volume.get(seed);
    if seed_color == EMPTY_VOXEL {
        return Ok(Vec::new());
    }
    let matches = |v| {
        v != EMPTY_VOXEL
            && (!opts.use_color || colors_match(palette, v, seed_color, opts.threshold))
    };
    if opts.entire_layer {
        let out: Vec<IVec3> = volume
            .iter()
            .filter(|&(_, v)| matches(v))
            .map(|(p, _)| p)
            .collect();
        return match out.len() > MAX_FLOOD_CELLS {
            true => Err(FloodCapped {
                limit: MAX_FLOOD_CELLS,
            }),
            false => Ok(out),
        };
    }
    flood(seed, MAX_FLOOD_CELLS, |p| matches(volume.get(p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::{BoundSize, Rgba8};

    /// Row x = 0..6 at y = z = 0: colors 1 1 2 1 _ 1. Palette 1 and 2 differ
    /// by 10 in red.
    fn row() -> (VoxelVolume, Palette) {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        for (x, c) in [(0, 1), (1, 1), (2, 2), (3, 1), (5, 1)] {
            v.set(IVec3::new(x, 0, 0), c);
        }
        let mut p = Palette::default();
        p.set(1, Rgba8::rgb(100, 50, 50));
        p.set(2, Rgba8::rgb(110, 50, 50));
        (v, p)
    }

    #[test]
    fn threshold_boundaries() {
        let (v, p) = row();
        let mut o = FuzzySelectOptions::default();
        let sel = |o: &FuzzySelectOptions| fuzzy_select(&v, IVec3::ZERO, &p, o).unwrap().len();
        // Exact color: stops at the different voxel at x = 2.
        assert_eq!(sel(&o), 2);
        o.threshold = 9;
        assert_eq!(sel(&o), 2, "10 apart is above 9");
        o.threshold = 10;
        assert_eq!(sel(&o), 4, "through x = 2 up to the gap at x = 4");
        o.use_color = false;
        o.threshold = 0;
        assert_eq!(sel(&o), 4);
        o.use_color = true;
        o.entire_layer = true;
        assert_eq!(sel(&o), 4, "every color-1 voxel, gap or not");
        assert!(fuzzy_select(&v, IVec3::new(4, 0, 0), &p, &o)
            .unwrap()
            .is_empty());
    }
}
