//! Per-layer voxel selection (Selection / Fuzzy Select tools).
//!
//! A selection is a sparse set of cells with a cached bounding box. It is
//! view/editing state: undoable through [`crate::Edit::SetSelection`] but
//! never written to `.rzxl` files.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::IVec3;
use rizxel_engine::tools::NEIGHBORS_6;
use rizxel_engine::MaskMode;
use rizxel_math::{BoundSize, VoxelVolume, EMPTY_VOXEL};

/// Source of [`Selection::revision`]s; unique for the process lifetime.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

/// A set of selected cells. Equality compares the cells only.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    cells: HashSet<IVec3>,
    /// `(min, max exclusive)` of `cells`, kept up to date on every change.
    bounds: Option<(IVec3, IVec3)>,
    /// Changes whenever the cells change (`0` for a default selection), so
    /// renderers can cache per revision. Clones keep it.
    revision: u64,
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells
    }
}

impl Eq for Selection {}

impl FromIterator<IVec3> for Selection {
    fn from_iter<I: IntoIterator<Item = IVec3>>(iter: I) -> Self {
        let mut s = Selection::default();
        s.extend(iter);
        s
    }
}

impl Extend<IVec3> for Selection {
    fn extend<I: IntoIterator<Item = IVec3>>(&mut self, iter: I) {
        let mut changed = false;
        for p in iter {
            if self.cells.insert(p) {
                self.grow_bounds(p);
                changed = true;
            }
        }
        if changed {
            self.revision = next_revision();
        }
    }
}

impl Selection {
    pub fn insert(&mut self, p: IVec3) -> bool {
        let added = self.cells.insert(p);
        if added {
            self.grow_bounds(p);
            self.revision = next_revision();
        }
        added
    }

    pub fn remove(&mut self, p: IVec3) -> bool {
        let removed = self.cells.remove(&p);
        if removed {
            // Only a cell on the box's surface can shrink it.
            if self
                .bounds
                .is_some_and(|(lo, hi)| p.cmpeq(lo).any() || (p + IVec3::ONE).cmpeq(hi).any())
            {
                self.recompute_bounds();
            }
            self.revision = next_revision();
        }
        removed
    }

    pub fn contains(&self, p: IVec3) -> bool {
        self.cells.contains(&p)
    }

    pub fn clear(&mut self) {
        if !self.cells.is_empty() {
            self.cells.clear();
            self.bounds = None;
            self.revision = next_revision();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Box around the selected cells: `(min, max exclusive)`.
    pub fn bounds(&self) -> Option<(IVec3, IVec3)> {
        self.bounds
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn iter(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.cells.iter().copied()
    }

    pub fn cells(&self) -> &HashSet<IVec3> {
        &self.cells
    }

    /// `self` combined with a freshly computed selection `other`: `Set`
    /// replaces, `Add` unites, `Subtract` removes `other`'s cells.
    pub fn combine(&self, other: &Selection, mode: MaskMode) -> Selection {
        match mode {
            MaskMode::Set => other.clone(),
            MaskMode::Add => {
                let mut out = self.clone();
                out.extend(other.iter());
                out
            }
            MaskMode::Subtract => self.iter().filter(|p| !other.contains(*p)).collect(),
        }
    }

    /// Selected solid cells of `volume` with at least one exposed face (an
    /// empty or out-of-bounds neighbour): what is visible of the selection.
    pub fn visible_shell(&self, volume: &VoxelVolume) -> Vec<IVec3> {
        self.iter()
            .filter(|&p| {
                volume.get(p) != EMPTY_VOXEL
                    && NEIGHBORS_6
                        .iter()
                        .any(|&n| volume.get(p + n) == EMPTY_VOXEL)
            })
            .collect()
    }

    /// Faces between a selected and an unselected cell, as
    /// `(selected cell, outward normal)`: the outline of the selection.
    pub fn boundary_faces(&self) -> Vec<(IVec3, IVec3)> {
        self.iter()
            .flat_map(|p| {
                NEIGHBORS_6
                    .iter()
                    .filter(move |&&n| !self.contains(p + n))
                    .map(move |&n| (p, n))
            })
            .collect()
    }

    /// Copy moved by `offset`; cells leaving `bounds` are dropped.
    pub fn translated(&self, offset: IVec3, bounds: BoundSize) -> Selection {
        let size = bounds.0.as_ivec3();
        self.iter()
            .map(|p| p + offset)
            .filter(|p| p.cmpge(IVec3::ZERO).all() && p.cmplt(size).all())
            .collect()
    }

    fn grow_bounds(&mut self, p: IVec3) {
        let cell = (p, p + IVec3::ONE);
        self.bounds = Some(match self.bounds {
            Some((lo, hi)) => (lo.min(cell.0), hi.max(cell.1)),
            None => cell,
        });
    }

    fn recompute_bounds(&mut self) {
        self.bounds = self
            .cells
            .iter()
            .map(|&p| (p, p + IVec3::ONE))
            .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(xs: &[i32]) -> Selection {
        xs.iter().map(|&x| IVec3::new(x, 0, 0)).collect()
    }

    #[test]
    fn combine_set_add_subtract() {
        let a = sel(&[0, 1, 2]);
        let b = sel(&[2, 3]);
        assert_eq!(a.combine(&b, MaskMode::Set), b);
        assert_eq!(a.combine(&b, MaskMode::Add), sel(&[0, 1, 2, 3]));
        assert_eq!(a.combine(&b, MaskMode::Subtract), sel(&[0, 1]));
        assert_eq!(
            Selection::default().combine(&b, MaskMode::Subtract),
            sel(&[])
        );
    }

    #[test]
    fn bounds_follow_inserts_and_removes() {
        let mut s = Selection::default();
        assert_eq!(s.bounds(), None);
        let r0 = s.revision();
        s.insert(IVec3::new(2, 3, 4));
        assert_eq!(s.bounds(), Some((IVec3::new(2, 3, 4), IVec3::new(3, 4, 5))));
        s.insert(IVec3::new(-1, 5, 4));
        assert_eq!(
            s.bounds(),
            Some((IVec3::new(-1, 3, 4), IVec3::new(3, 6, 5)))
        );
        assert_ne!(s.revision(), r0);
        let r1 = s.revision();
        assert!(!s.insert(IVec3::new(2, 3, 4)), "already selected");
        assert_eq!(s.revision(), r1, "no change, same revision");
        // Removing an extreme cell shrinks the box; an inner one does not.
        s.insert(IVec3::new(0, 4, 4));
        s.remove(IVec3::new(-1, 5, 4));
        assert_eq!(s.bounds(), Some((IVec3::new(0, 3, 4), IVec3::new(3, 5, 5))));
        s.remove(IVec3::new(0, 4, 4));
        s.remove(IVec3::new(2, 3, 4));
        assert_eq!(s.bounds(), None);
        assert!(s.is_empty());
    }

    #[test]
    fn shell_of_a_solid_cube_skips_the_core() {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        let mut s = Selection::default();
        for z in 1..4 {
            for y in 1..4 {
                for x in 1..4 {
                    v.set(IVec3::new(x, y, z), 1);
                    s.insert(IVec3::new(x, y, z));
                }
            }
        }
        s.insert(IVec3::splat(6)); // empty cell: never part of the shell
        assert_eq!(s.visible_shell(&v).len(), 26);
        assert!(!s.visible_shell(&v).contains(&IVec3::splat(2)));
        // 3³ selected cube: 9 faces per side; the lone cell adds 6.
        assert_eq!(s.boundary_faces().len(), 6 * 9 + 6);
    }

    #[test]
    fn translate_clips_to_bounds() {
        let s = sel(&[0, 1, 7]);
        let moved = s.translated(IVec3::X, BoundSize::new(8, 8, 8));
        assert_eq!(moved, sel(&[1, 2]));
    }
}
