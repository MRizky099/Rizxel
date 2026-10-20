//! Color reduction for importing true-color formats (Goxel, textured or
//! vertex-colored meshes) into Rizxel's 255-color palette.

use std::collections::HashMap;

use rizxel_math::{Palette, Rgba8, Voxel, MAX_PALETTE_COLORS};

/// Most paint colors a palette can hold (slot 0 is the reserved empty slot).
pub const MAX_PAINT_COLORS: usize = MAX_PALETTE_COLORS - 1;

/// Palette built from imported colors plus the voxel value for each color.
#[derive(Debug, Clone)]
pub struct ColorMap {
    pub palette: Palette,
    lookup: HashMap<Rgba8, Voxel>,
}

impl ColorMap {
    /// Voxel value for an input color (nearest palette color when the input
    /// was reduced).
    pub fn value(&self, color: Rgba8) -> Voxel {
        let key = opaque(color);
        self.lookup
            .get(&key)
            .copied()
            .unwrap_or_else(|| nearest(&self.palette, key))
    }
}

fn opaque(c: Rgba8) -> Rgba8 {
    Rgba8::rgb(c.r, c.g, c.b)
}

/// Builds a palette for `colors` (with use counts, in first-seen order).
/// Up to 255 distinct colors are kept exactly, in that order, at indices
/// `1..`; more are reduced with median cut. Alpha is ignored (voxels are
/// opaque). Slot 0 holds black as the reserved empty slot.
pub fn build_color_map(colors: &[(Rgba8, u64)]) -> ColorMap {
    let mut merged: Vec<(Rgba8, u64)> = Vec::new();
    let mut seen: HashMap<Rgba8, usize> = HashMap::new();
    for &(c, n) in colors {
        let c = opaque(c);
        match seen.get(&c) {
            Some(&i) => merged[i].1 += n,
            None => {
                seen.insert(c, merged.len());
                merged.push((c, n));
            }
        }
    }
    let paint: Vec<Rgba8> = if merged.len() <= MAX_PAINT_COLORS {
        merged.iter().map(|&(c, _)| c).collect()
    } else {
        median_cut(merged, MAX_PAINT_COLORS)
    };
    let mut entries = vec![Rgba8::BLACK];
    entries.extend(paint);
    let palette = Palette::new(entries);
    let lookup = seen.keys().map(|&c| (c, nearest(&palette, c))).collect();
    ColorMap { palette, lookup }
}

/// Palette index (never 0) closest to `c` in RGB.
pub fn nearest(palette: &Palette, c: Rgba8) -> Voxel {
    let dist = |p: Rgba8| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(p.r, c.r) + d(p.g, c.g) + d(p.b, c.b)
    };
    palette
        .colors()
        .iter()
        .enumerate()
        .skip(1)
        .min_by_key(|(_, &p)| dist(p))
        .map(|(i, _)| i as Voxel)
        .unwrap_or(1)
}

/// Median cut: splits the box with the widest channel range at its weighted
/// median until there are `target` boxes, then averages each box.
fn median_cut(colors: Vec<(Rgba8, u64)>, target: usize) -> Vec<Rgba8> {
    let channel = |c: Rgba8, k: usize| [c.r, c.g, c.b][k];
    let range = |b: &[(Rgba8, u64)]| -> (usize, u8) {
        (0..3)
            .map(|k| {
                let lo = b.iter().map(|&(c, _)| channel(c, k)).min().unwrap_or(0);
                let hi = b.iter().map(|&(c, _)| channel(c, k)).max().unwrap_or(0);
                (k, hi - lo)
            })
            .max_by_key(|&(_, r)| r)
            .unwrap_or((0, 0))
    };
    let mut boxes: Vec<Vec<(Rgba8, u64)>> = vec![colors];
    while boxes.len() < target {
        // Split the box with the widest range (ties: most colors).
        let Some((i, (k, r))) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, range(b)))
            .max_by_key(|&(i, (_, r))| (r, boxes[i].len()))
        else {
            break;
        };
        if r == 0 {
            break;
        }
        let mut b = boxes.swap_remove(i);
        b.sort_by_key(|&(c, _)| channel(c, k));
        let total: u64 = b.iter().map(|&(_, n)| n).sum();
        let mut acc = 0;
        let mut split = b.len() / 2;
        for (j, &(_, n)) in b.iter().enumerate() {
            acc += n;
            if acc * 2 >= total {
                split = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(split);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let total: u64 = b.iter().map(|&(_, n)| n.max(1)).sum::<u64>().max(1);
            let avg = |k: usize| {
                (b.iter()
                    .map(|&(c, n)| channel(c, k) as u64 * n.max(1))
                    .sum::<u64>()
                    / total) as u8
            };
            Rgba8::rgb(avg(0), avg(1), avg(2))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn few_colors_are_kept_exactly_in_order() {
        let red = Rgba8::rgb(255, 0, 0);
        let blue = Rgba8::rgb(0, 0, 255);
        let m = build_color_map(&[(red, 3), (blue, 1), (Rgba8::new(255, 0, 0, 10), 2)]);
        assert_eq!(m.palette.len(), 3);
        assert_eq!(m.value(red), 1);
        assert_eq!(m.value(blue), 2);
        assert_eq!(m.value(Rgba8::new(255, 0, 0, 0)), 1, "alpha ignored");
    }

    #[test]
    fn many_colors_are_reduced_to_255() {
        let colors: Vec<_> = (0..4096u32)
            .map(|i| {
                (
                    Rgba8::rgb(
                        (i % 16 * 16) as u8,
                        (i / 16 % 16 * 16) as u8,
                        (i / 256 * 16) as u8,
                    ),
                    1,
                )
            })
            .collect();
        let m = build_color_map(&colors);
        assert_eq!(m.palette.len(), 256);
        for &(c, _) in colors.iter().step_by(97) {
            let got = m.palette.get(m.value(c) as usize).unwrap();
            let err = (got.r as i32 - c.r as i32).abs()
                + (got.g as i32 - c.g as i32).abs()
                + (got.b as i32 - c.b as i32).abs();
            assert!(err <= 96, "{c:?} -> {got:?}");
            assert_ne!(m.value(c), 0);
        }
    }
}
