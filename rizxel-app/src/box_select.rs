//! Box selection session (Selection tool, *Box* mode): a box that stays
//! open after the drag, resized by handles or typed values, whose cells
//! are re-selected live as a preview. Applying it is one undo step
//! (`Edit::SetSelection`); cancelling leaves the selection untouched.

use rizxel_engine::tools::{box_cells, fit_box, select_box, MAX_LIVE_BOX};
use rizxel_engine::MaskMode;
use rizxel_math::VoxelBox;

use crate::document::Document;
use crate::selection::Selection;

/// An open box selection on one layer.
#[derive(Debug, Clone)]
pub struct BoxSelect {
    /// Layer whose selection the box edits (the active layer at start).
    pub layer: usize,
    pub area: VoxelBox,
    /// Select the voxels of every visible layer inside the box.
    pub all_layers: bool,
    /// How the box combines with the selection (the modifiers held).
    pub mode: MaskMode,
    /// Selection the box combines with; `None` uses the layer's current one.
    base: Option<Selection>,
    /// Solid cells inside `area`.
    cells: Selection,
    /// What applying gives: the base combined with `cells` by `mode`.
    preview: Selection,
    /// `cells` were not recomputed for the last change of a large box.
    pub stale: bool,
}

impl BoxSelect {
    /// Number of cells inside the box that would be selected.
    pub fn found(&self) -> usize {
        self.cells.len()
    }
}

/// The box of the last applied box selection, drawn dimmed until the
/// selection changes. Clicking one of its handles re-opens it.
#[derive(Debug, Clone)]
pub struct LastBox {
    pub layer: usize,
    pub area: VoxelBox,
    pub all_layers: bool,
    /// Selection revision right after applying.
    revision: u64,
    /// Selection the box was combined with.
    base: Selection,
}

impl Document {
    /// Area a box may cover: the bounds, or the slice layer while slicing.
    pub fn box_limit(&self) -> VoxelBox {
        self.edit_clip()
            .unwrap_or(VoxelBox::from_bounds(self.data.bounds))
    }

    /// Opens a box selection on the active layer (an open one is applied
    /// first).
    pub fn begin_box_select(&mut self, area: VoxelBox, all_layers: bool, mode: MaskMode) {
        self.apply_box_select();
        self.open_box(area, all_layers, mode, None);
    }

    fn open_box(
        &mut self,
        area: VoxelBox,
        all_layers: bool,
        mode: MaskMode,
        base: Option<Selection>,
    ) {
        let area = fit_box(area, &self.box_limit());
        self.box_select = Some(BoxSelect {
            layer: self.active_layer,
            area,
            all_layers,
            mode,
            base,
            cells: Selection::default(),
            preview: Selection::default(),
            stale: true,
        });
        self.refresh_box_select();
    }

    /// Re-opens the last applied box (its handle was grabbed). `false` when
    /// it is no longer shown.
    pub fn reopen_last_box(&mut self, mode: MaskMode) -> bool {
        let Some(last) = self.last_box() else {
            return false;
        };
        let (area, all, base) = (last.area, last.all_layers, last.base.clone());
        self.open_box(area, all, mode, Some(base));
        true
    }

    pub fn box_select(&self) -> Option<&BoxSelect> {
        self.box_select.as_ref()
    }

    /// The last applied box while the selection it made is unchanged.
    pub fn last_box(&self) -> Option<&LastBox> {
        let last = self.last_box.as_ref()?;
        let sel = self.data.layers.get(last.layer)?.selection.revision();
        (self.box_select.is_none() && last.layer == self.active_layer && sel == last.revision)
            .then_some(last)
    }

    /// Selection to draw for the active layer: the box preview while a box
    /// is open, else the layer's selection.
    pub fn shown_selection(&self) -> Option<&Selection> {
        match &self.box_select {
            Some(b) if b.layer == self.active_layer => Some(&b.preview),
            _ => self.selection(),
        }
    }

    /// Moves or resizes the open box (clamped into [`Document::box_limit`]).
    /// With `live` false, boxes over [`MAX_LIVE_BOX`] cells keep their old
    /// preview until [`Document::refresh_box_select`].
    pub fn set_box_area(&mut self, area: VoxelBox, live: bool) {
        let limit = self.box_limit();
        let Some(b) = &mut self.box_select else {
            return;
        };
        let area = fit_box(area, &limit);
        if b.area == area && !b.stale {
            return;
        }
        b.area = area;
        b.stale = true;
        if live || box_cells(&area) <= MAX_LIVE_BOX {
            self.refresh_box_select();
        }
    }

    /// Changes how the open box combines with the selection.
    pub fn set_box_mode(&mut self, mode: MaskMode) {
        if let Some(b) = &mut self.box_select {
            if b.mode != mode {
                b.mode = mode;
                self.update_box_preview();
            }
        }
    }

    pub fn set_box_all_layers(&mut self, all_layers: bool) {
        if let Some(b) = &mut self.box_select {
            if b.all_layers != all_layers {
                b.all_layers = all_layers;
                self.refresh_box_select();
            }
        }
    }

    /// Recomputes the cells inside the open box.
    pub fn refresh_box_select(&mut self) {
        let Some(b) = &self.box_select else { return };
        let (area, layer, all) = (b.area, b.layer, b.all_layers);
        let mut cells = if all {
            self.with_visible_geometry(|vols| select_box(vols, &area))
        } else {
            self.data
                .layers
                .get(layer)
                .map(|l| select_box(&[&l.volume], &area))
                .unwrap_or_default()
        };
        if let Some(clip) = self.edit_clip() {
            cells.retain(|p| clip.contains(*p));
        }
        if let Some(b) = &mut self.box_select {
            b.cells = cells.into_iter().collect();
            b.stale = false;
        }
        self.update_box_preview();
    }

    fn update_box_preview(&mut self) {
        let Some(b) = &self.box_select else { return };
        let current = self.data.layers.get(b.layer).map(|l| &l.selection);
        let base = b.base.as_ref().or(current);
        let preview = base.map_or_else(|| b.cells.clone(), |base| base.combine(&b.cells, b.mode));
        if let Some(b) = &mut self.box_select {
            b.preview = preview;
        }
    }

    /// Applies the open box as the layer's selection, as one undo step.
    /// Returns whether the selection changed.
    pub fn apply_box_select(&mut self) -> bool {
        if self.box_select.as_ref().is_some_and(|b| b.stale) {
            self.refresh_box_select();
        }
        let Some(b) = self.box_select.take() else {
            return false;
        };
        let Some(current) = self.data.layers.get(b.layer).map(|l| l.selection.clone()) else {
            return false;
        };
        let base = b.base.unwrap_or(current);
        let new = base.combine(&b.cells, b.mode);
        let changed = self.labeled("Box Select", |d| d.set_selection(b.layer, new));
        self.last_box = self.data.layers.get(b.layer).map(|l| LastBox {
            layer: b.layer,
            area: b.area,
            all_layers: b.all_layers,
            revision: l.selection.revision(),
            base,
        });
        changed
    }

    /// Closes the open box without changing the selection.
    pub fn cancel_box_select(&mut self) -> bool {
        self.box_select.take().is_some()
    }

    /// Drops an open box whose layer is no longer the active one, or no
    /// longer exists (layer switch, undo of a layer insert).
    pub fn sync_box_select(&mut self) {
        let active = self.active_layer;
        let gone = self
            .box_select
            .as_ref()
            .is_some_and(|b| b.layer != active || b.layer >= self.data.layers.len());
        if gone {
            self.box_select = None;
        }
    }
}
