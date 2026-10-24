//! Layer Booleans (Qubicle *Boolean*): the active layer combined with
//! another layer's geometry. The result replaces the active layer; the
//! other layer is only read.

use rizxel_math::{VoxelVolume, EMPTY_VOXEL};

use super::VoxelChange;

/// How [`boolean_changes`] combines the two layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    /// Adds the other layer's voxels where the active layer is empty (the
    /// active layer's colors win on overlaps).
    Union,
    /// Removes the active layer's voxels where the other layer is solid.
    Difference,
    /// Keeps only the active layer's voxels where the other layer is solid.
    Intersection,
}

impl BooleanOp {
    pub const ALL: [BooleanOp; 3] = [
        BooleanOp::Union,
        BooleanOp::Difference,
        BooleanOp::Intersection,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            BooleanOp::Union => "Union",
            BooleanOp::Difference => "Difference",
            BooleanOp::Intersection => "Intersection",
        }
    }
}

/// Changes that turn `active` into `active op other`. Cells of `other`
/// outside `active`'s bounds are ignored.
pub fn boolean_changes(
    active: &VoxelVolume,
    other: &VoxelVolume,
    op: BooleanOp,
) -> Vec<VoxelChange> {
    let clear = |pos, old| VoxelChange {
        pos,
        old,
        new: EMPTY_VOXEL,
    };
    match op {
        BooleanOp::Union => other
            .iter()
            .filter(|&(p, _)| active.in_bounds(p) && active.get(p) == EMPTY_VOXEL)
            .map(|(pos, new)| VoxelChange {
                pos,
                old: EMPTY_VOXEL,
                new,
            })
            .collect(),
        BooleanOp::Difference => active
            .iter()
            .filter(|&(p, _)| other.get(p) != EMPTY_VOXEL)
            .map(|(p, v)| clear(p, v))
            .collect(),
        BooleanOp::Intersection => active
            .iter()
            .filter(|&(p, _)| other.get(p) == EMPTY_VOXEL)
            .map(|(p, v)| clear(p, v))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;
    use rizxel_math::BoundSize;

    /// Truth table over four cells: (active, other) = (0,0) (1,0) (0,1) (1,1).
    #[test]
    fn truth_tables() {
        let b = BoundSize::new(4, 1, 1);
        let mut a = VoxelVolume::new(b);
        let mut o = VoxelVolume::new(b);
        a.set(IVec3::new(1, 0, 0), 2);
        a.set(IVec3::new(3, 0, 0), 2);
        o.set(IVec3::new(2, 0, 0), 5);
        o.set(IVec3::new(3, 0, 0), 5);
        let result = |op| {
            let mut v = a.clone();
            for c in boolean_changes(&a, &o, op) {
                v.set(c.pos, c.new);
            }
            (0..4)
                .map(|x| v.get(IVec3::new(x, 0, 0)))
                .collect::<Vec<_>>()
        };
        assert_eq!(result(BooleanOp::Union), [0, 2, 5, 2]);
        assert_eq!(result(BooleanOp::Difference), [0, 2, 0, 0]);
        assert_eq!(result(BooleanOp::Intersection), [0, 0, 0, 2]);
    }
}
