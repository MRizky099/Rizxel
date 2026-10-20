//! Mirror transform shared by the drawing tools (Goxel's symmetry axes).

use std::collections::HashSet;

use glam::IVec3;
use rizxel_math::BoundSize;

use super::Symmetry;

/// Mirrors `p` across the center plane of the bounds on `axis`.
pub fn mirror(p: IVec3, bounds: BoundSize, axis: usize) -> IVec3 {
    let mut out = p;
    out[axis] = bounds.0[axis] as i32 - 1 - p[axis];
    out
}

/// Adds the mirror images of `positions` for every enabled axis (up to 8
/// copies with all three axes) and removes duplicates, keeping first-seen
/// order so the output is deterministic.
pub fn apply_symmetry(positions: Vec<IVec3>, bounds: BoundSize, sym: Symmetry) -> Vec<IVec3> {
    let axes: Vec<usize> = [sym.x, sym.y, sym.z]
        .iter()
        .enumerate()
        .filter_map(|(i, &on)| on.then_some(i))
        .collect();
    if axes.is_empty() {
        return positions;
    }
    let mut out = positions;
    for axis in axes {
        let mirrored: Vec<IVec3> = out.iter().map(|&p| mirror(p, bounds, axis)).collect();
        out.extend(mirrored);
    }
    let mut seen = HashSet::with_capacity(out.len());
    out.retain(|p| seen.insert(*p));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_on_each_enabled_axis() {
        let b = BoundSize::new(8, 4, 6);
        let p = IVec3::new(1, 0, 2);
        assert_eq!(mirror(p, b, 0), IVec3::new(6, 0, 2));
        assert_eq!(mirror(p, b, 2), IVec3::new(1, 0, 3));
        let none = apply_symmetry(vec![p], b, Symmetry::default());
        assert_eq!(none, vec![p]);
        let all = Symmetry {
            x: true,
            y: true,
            z: true,
        };
        assert_eq!(apply_symmetry(vec![p], b, all).len(), 8);
        // A cell on the mirror plane of an odd axis maps onto itself.
        let odd = BoundSize::new(5, 5, 5);
        let x_only = Symmetry {
            x: true,
            ..Symmetry::default()
        };
        assert_eq!(
            apply_symmetry(vec![IVec3::new(2, 1, 1)], odd, x_only).len(),
            1
        );
    }
}
