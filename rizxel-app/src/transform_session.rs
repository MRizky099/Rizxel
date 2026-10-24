//! Transform tool session: the selection (or the whole active layer) is
//! lifted into an open edit transaction, like a floating paste, and
//! redrawn transformed on every change of its parameters. Applying it is
//! one undo step that carries the selection along; cancelling restores the
//! original voxels and records nothing.

use std::collections::HashMap;

use glam::{IVec3, Vec3};
use rizxel_engine::tools::{apply_transform, cells_box, TransformParams};
use rizxel_engine::Pivot;
use rizxel_math::{Voxel, VoxelBox, EMPTY_VOXEL};

use crate::document::{Document, TransformError};
use crate::history::VoxelChange;
use crate::selection::Selection;

/// An open transform of one layer's voxels.
#[derive(Debug, Clone)]
pub struct TransformSession {
    pub layer: usize,
    /// The lifted voxels at their original place.
    source: Vec<(IVec3, Voxel)>,
    /// Box of the source: minimum corner and size.
    pub min: IVec3,
    pub size: IVec3,
    /// The layer's selection when the session started (`None`: the whole
    /// layer is transformed).
    selection: Option<Selection>,
    pub params: TransformParams,
    pub pivot: Pivot,
    /// Voxel used by [`Pivot::Cursor`].
    pub pivot_cell: IVec3,
    /// Transformed voxels, including those outside the bounds.
    result: Vec<(IVec3, Voxel)>,
    /// Transformed voxels that fall outside the bounds.
    pub out_of_bounds: Vec<IVec3>,
}

impl TransformSession {
    /// Pivot point in voxel space.
    pub fn pivot_point(&self) -> Vec3 {
        pivot_point(self.pivot, self.min, self.size, self.pivot_cell)
    }

    /// Box around the transformed voxels (`min`, `max` exclusive).
    pub fn result_box(&self) -> Option<VoxelBox> {
        cells_box(self.result.iter().map(|c| c.0)).map(|(min, size)| VoxelBox {
            min,
            max: min + size,
        })
    }

    pub fn voxel_count(&self) -> usize {
        self.source.len()
    }
}

/// Pivot point of a box `(min, size)` for `pivot`.
pub fn pivot_point(pivot: Pivot, min: IVec3, size: IVec3, cell: IVec3) -> Vec3 {
    match pivot {
        Pivot::Center => min.as_vec3() + size.as_vec3() * 0.5,
        Pivot::MinCorner => min.as_vec3(),
        Pivot::Cursor => cell.as_vec3() + Vec3::splat(0.5),
    }
}

/// What the Transform tool acts on.
struct Source {
    cells: Vec<(IVec3, Voxel)>,
    min: IVec3,
    size: IVec3,
    selection: Option<Selection>,
}

impl Document {
    /// What the Transform tool would act on: the selected voxels (or every
    /// voxel of the active layer) and their box.
    fn transform_source(&self) -> Option<Source> {
        let l = self.data.layers.get(self.active_layer)?;
        let selection = (!l.selection.is_empty()).then(|| l.selection.clone());
        let source: Vec<(IVec3, Voxel)> = match &selection {
            Some(sel) => sel
                .iter()
                .map(|p| (p, l.volume.get(p)))
                .filter(|&(_, v)| v != EMPTY_VOXEL)
                .collect(),
            None => l.volume.iter().collect(),
        };
        let (min, size) = match selection.as_ref().and_then(Selection::bounds) {
            Some((lo, hi)) => (lo, hi - lo),
            None => cells_box(source.iter().map(|c| c.0))?,
        };
        (!source.is_empty()).then_some(Source {
            cells: source,
            min,
            size,
            selection,
        })
    }

    /// Box the Transform tool would act on, before a session starts.
    pub fn transform_target_box(&self) -> Option<VoxelBox> {
        let s = self.transform_source()?;
        Some(VoxelBox {
            min: s.min,
            max: s.min + s.size,
        })
    }

    /// Lifts the selection (or the active layer) into a transform session
    /// about `pivot`. An open one is kept.
    pub fn begin_transform(
        &mut self,
        pivot: Pivot,
        pivot_cell: IVec3,
    ) -> Result<(), TransformError> {
        if self.transform.is_some() {
            return Ok(());
        }
        let layer = self.transform_layer()?;
        let Source {
            cells: source,
            min,
            size,
            selection,
        } = self.transform_source().ok_or(TransformError::Empty)?;
        self.cancel_paste();
        self.cancel_edit();
        self.begin_edit();
        self.transform = Some(TransformSession {
            layer,
            result: source.clone(),
            source,
            min,
            size,
            selection,
            params: TransformParams::default(),
            pivot,
            pivot_cell,
            out_of_bounds: Vec::new(),
        });
        Ok(())
    }

    pub fn transform_session(&self) -> Option<&TransformSession> {
        self.transform.as_ref()
    }

    /// Redraws the open transform with `params` about `pivot`.
    pub fn set_transform(&mut self, params: TransformParams, pivot: Pivot, pivot_cell: IVec3) {
        let bounds = VoxelBox::from_bounds(self.data.bounds);
        let Some(s) = &mut self.transform else { return };
        if s.params == params && s.pivot == pivot && s.pivot_cell == pivot_cell {
            return;
        }
        s.params = params;
        s.pivot = pivot;
        s.pivot_cell = pivot_cell;
        s.result = apply_transform(&s.source, &params, s.pivot_point());
        s.out_of_bounds = s
            .result
            .iter()
            .map(|c| c.0)
            .filter(|p| !bounds.contains(*p))
            .collect();
        let layer = s.layer;
        let mut writes: HashMap<IVec3, Voxel> =
            s.source.iter().map(|&(p, _)| (p, EMPTY_VOXEL)).collect();
        writes.extend(
            s.result
                .iter()
                .copied()
                .filter(|(p, _)| bounds.contains(*p)),
        );
        self.revert_pending();
        let changes: Vec<VoxelChange> = writes
            .into_iter()
            .map(|(pos, new)| VoxelChange {
                pos,
                old: EMPTY_VOXEL,
                new,
            })
            .collect();
        self.edit_voxels(layer, &changes);
    }

    /// Applies the open transform as one undo step (none when nothing
    /// changed). The selection follows the voxels. Refused while voxels
    /// would leave the bounds, unless `clip` drops them.
    pub fn apply_transform(&mut self, clip: bool) -> Result<usize, TransformError> {
        let Some(s) = &self.transform else {
            return Err(TransformError::Empty);
        };
        if !s.out_of_bounds.is_empty() && !clip {
            return Err(TransformError::OutOfBounds);
        }
        let Some(s) = self.transform.take() else {
            return Err(TransformError::Empty);
        };
        let bounds = VoxelBox::from_bounds(self.data.bounds);
        let changed = self.pending_changes();
        let done = match s.selection {
            Some(_) => {
                let moved: Selection = s
                    .result
                    .iter()
                    .map(|c| c.0)
                    .filter(|p| bounds.contains(*p))
                    .collect();
                self.labeled("Transform", |d| {
                    d.commit_edit_with_selection(s.layer, moved)
                })
            }
            None => self.labeled("Transform", Document::commit_edit),
        };
        Ok(if done { changed } else { 0 })
    }

    /// Drops the open transform and restores the voxels.
    pub fn cancel_transform(&mut self) -> bool {
        if self.transform.is_none() {
            return false;
        }
        self.cancel_edit();
        true
    }
}
