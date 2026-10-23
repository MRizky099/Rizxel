//! Phase 10: selection operations and Mask Mode, structure operations
//! (Separate Parts, Fit Bounds, Fill Hollows / Hollow, Booleans), palette
//! tools and the undo history list.

use glam::IVec3;
use rizxel_app::{AppState, Document, Layer, Selection, TransformError, VoxelChange};
use rizxel_engine::tools::{apply_op, extrude_changes, extrude_region, fill_cells, Transform};
use rizxel_engine::{ExtrudeOptions, FillMode, FillOptions, MaskMode, RayHit, VoxelOp};
use rizxel_math::{BoundSize, Voxel, VoxelBox, EMPTY_VOXEL};

/// Solid box `lo..hi` of color `c`.
fn box_layer(name: &str, lo: IVec3, hi: IVec3, c: Voxel) -> Layer {
    let mut l = Layer::new(name, BoundSize::default());
    fill_box(&mut l, lo, hi, c);
    l
}

fn fill_box(l: &mut Layer, lo: IVec3, hi: IVec3, c: Voxel) {
    for z in lo.z..hi.z {
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                l.volume.set(IVec3::new(x, y, z), c);
            }
        }
    }
}

fn cube_doc() -> Document {
    Document::with_layers("t", vec![box_layer("A", IVec3::ZERO, IVec3::splat(5), 2)])
}

fn cells(lo: IVec3, hi: IVec3) -> Selection {
    let mut s = Selection::default();
    for z in lo.z..hi.z {
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                s.insert(IVec3::new(x, y, z));
            }
        }
    }
    s
}

// ------------------------------------------------------------ Track A

#[test]
fn grow_and_shrink_are_single_undo_steps() {
    let mut d = cube_doc();
    let core = cells(IVec3::ONE, IVec3::splat(4));
    d.set_selection(0, core.clone());
    assert!(d.grow_selection(1));
    assert_eq!(d.selection().unwrap().len(), 27 + 6 * 9);
    assert!(d.shrink_selection(1));
    assert_eq!(d.selection(), Some(&core));
    d.undo();
    assert_eq!(d.selection().unwrap().len(), 27 + 6 * 9);
    d.undo();
    assert_eq!(d.selection(), Some(&core));
}

#[test]
fn invert_of_an_empty_selection_selects_every_voxel() {
    let mut d = cube_doc();
    assert!(d.invert_selection());
    assert_eq!(d.selection().unwrap().len(), 125);
    assert!(d.invert_selection());
    assert!(d.selection().unwrap().is_empty());
}

#[test]
fn select_by_color_combines_with_the_mask_mode() {
    let mut d = cube_doc();
    let mut l = Layer::new("B", BoundSize::default());
    l.volume.set(IVec3::ZERO, 2);
    l.volume.set(IVec3::X, 3);
    d.data.layers[0] = l;
    assert!(d.select_by_color(2, 0, MaskMode::Set));
    assert_eq!(d.selection().unwrap().len(), 1);
    assert!(d.select_by_color(3, 0, MaskMode::Add));
    assert_eq!(d.selection().unwrap().len(), 2);
}

/// Mask Mode on, with the cube's top layer (y = 4) selected.
fn masked_doc() -> Document {
    let mut d = cube_doc();
    d.set_selection(0, cells(IVec3::new(0, 4, 0), IVec3::splat(5)));
    d.mask_selection = true;
    d
}

#[test]
fn mask_mode_blocks_brush_writes_to_selected_cells() {
    let mut d = masked_doc();
    let paint = |p| VoxelChange {
        pos: p,
        old: 0,
        new: 7,
    };
    let n = d.edit_voxels(0, &[paint(IVec3::new(1, 4, 1)), paint(IVec3::new(1, 3, 1))]);
    assert_eq!(n, 1);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(1, 4, 1)), 2);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(1, 3, 1)), 7);
    // Without Mask Mode the selection is just a selection.
    d.mask_selection = false;
    assert_eq!(d.edit_voxels(0, &[paint(IVec3::new(1, 4, 1))]), 1);
}

#[test]
fn mask_mode_blocks_fill_and_extrude() {
    let mut d = masked_doc();
    let volume = d.data.layers[0].volume.clone();
    // Recolor fill of the whole cube changes the 100 unprotected voxels.
    let opts = FillOptions {
        mode: FillMode::Recolor,
        ..FillOptions::default()
    };
    let hit = RayHit {
        voxel: IVec3::ZERO,
        normal: IVec3::NEG_Y,
        t: 0.0,
    };
    let all = VoxelBox::from_bounds(d.data.bounds);
    let region = d.region_selection().map(|s| s.cells().clone());
    assert!(region.is_none(), "a protected selection is no region");
    let filled = fill_cells(&volume, &hit, &d.data.palette, &opts, &all, None).unwrap();
    let changes = apply_op(&volume, filled, VoxelOp::Add, 9);
    assert_eq!(d.edit_voxels(0, &changes), 100);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(2, 4, 2)), 2);

    // Extruding the top face inward (carve) leaves the protected layer.
    let volume = d.data.layers[0].volume.clone();
    let top = RayHit {
        voxel: IVec3::new(2, 4, 2),
        normal: IVec3::Y,
        t: 0.0,
    };
    let face = extrude_region(&volume, &top, &ExtrudeOptions::default(), &d.data.palette);
    let carve = extrude_changes(&volume, &volume, &face, IVec3::Y, -1);
    assert!(!carve.is_empty());
    assert_eq!(d.edit_voxels(0, &carve), 0);
    // Extruding the bottom face outward is fine.
    let bottom = RayHit {
        voxel: IVec3::new(2, 0, 2),
        normal: IVec3::NEG_Y,
        t: 0.0,
    };
    let face = extrude_region(
        &volume,
        &bottom,
        &ExtrudeOptions::default(),
        &d.data.palette,
    );
    let grow = extrude_changes(&volume, &volume, &face, IVec3::NEG_Y, -1);
    assert!(d.edit_voxels(0, &grow) > 0);
}

#[test]
fn mask_mode_refuses_commands_on_the_selection() {
    let mut d = masked_doc();
    assert_eq!(d.delete_selected(), 0);
    assert!(d.cut().is_none());
    assert_eq!(
        d.transform(Transform::Flip { axis: 0 }),
        Err(TransformError::Masked)
    );
    assert_eq!(d.mirror(0), Err(TransformError::Masked));
    assert!(!d.can_undo() || d.data.layers[0].volume.voxel_count() == 125);
    assert_eq!(d.data.layers[0].volume.voxel_count(), 125);
}

#[test]
fn mask_mode_follows_the_app_toggle() {
    let mut s = AppState::default();
    s.toggle_mask_selection();
    assert!(s.options.mask_selection);
    assert!(s.doc().unwrap().mask_selection);
    s.new_document();
    assert!(s.doc().unwrap().mask_selection, "new tabs follow it");
    s.toggle_mask_selection();
    assert!(s.documents.iter().all(|d| !d.mask_selection));
}

#[test]
fn mask_mode_keeps_slice_cuts_off_the_selection() {
    // One of the four cuts would remove only the protected top layer.
    let mut removed = 0;
    for (coord, flip) in [(1, false), (1, true), (4, false), (4, true)] {
        let mut d = masked_doc();
        d.plane.axis = 1;
        d.plane.coord = coord;
        d.plane.flip = flip;
        let out = d.cut_hidden_side(false, 1);
        let l = &d.data.layers[0];
        removed += out.removed;
        assert_eq!(125 - l.volume.voxel_count(), out.removed);
        assert!(l.selection.iter().all(|p| l.volume.get(p) != EMPTY_VOXEL));
    }
    assert!(removed > 0);
}
