//! Phase 10c: box selection, selection to a new layer, lines and curves,
//! and the Transform tool. The randomized undo/redo round trip over them
//! lives in `phase5b_tools.rs`.

use glam::IVec3;
use rizxel_app::{Document, Layer, Selection};
use rizxel_engine::MaskMode;
use rizxel_math::{BoundSize, Voxel, VoxelBox};

/// 4x4x4 solid of color `c` at the origin.
fn solid_layer(name: &str, c: Voxel) -> Layer {
    let mut l = Layer::new(name, BoundSize::default());
    for z in 0..4 {
        for y in 0..4 {
            for x in 0..4 {
                l.volume.set(IVec3::new(x, y, z), c);
            }
        }
    }
    l
}

fn area(min: [i32; 3], max: [i32; 3]) -> VoxelBox {
    VoxelBox {
        min: IVec3::from_array(min),
        max: IVec3::from_array(max),
    }
}

fn history_len(d: &Document) -> usize {
    d.history_entries().0.len()
}

// ------------------------------------------------------------ box select

#[test]
fn box_select_applies_in_one_step() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.begin_box_select(area([0, 0, 0], [1, 1, 1]), false, MaskMode::Set);
    // Live preview, nothing recorded yet.
    assert_eq!(d.box_select().unwrap().found(), 1);
    d.set_box_area(area([0, 0, 0], [2, 4, 1]), true);
    assert_eq!(d.shown_selection().unwrap().len(), 8);
    assert!(d.selection().unwrap().is_empty());
    assert_eq!(history_len(&d), 0);
    // A box moved past the bounds is pushed back inside.
    d.set_box_area(area([-5, 0, 0], [-3, 4, 1]), true);
    assert_eq!(d.box_select().unwrap().area, area([0, 0, 0], [2, 4, 1]));

    assert!(d.apply_box_select());
    assert_eq!(history_len(&d), 1);
    assert_eq!(d.history_entries().0[0], "Box Select");
    assert_eq!(d.selection().unwrap().len(), 8);
    assert!(d.box_select().is_none());
    // The applied box is still shown until the selection changes.
    assert_eq!(d.last_box().unwrap().area, area([0, 0, 0], [2, 4, 1]));
    assert!(d.undo());
    assert!(d.selection().unwrap().is_empty());
    assert!(d.last_box().is_none());
}

#[test]
fn box_select_cancel_restores_the_selection_exactly() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    let before: Selection = [IVec3::new(3, 3, 3), IVec3::new(2, 3, 3)]
        .into_iter()
        .collect();
    d.select(before.iter(), MaskMode::Set);
    let steps = history_len(&d);
    d.begin_box_select(area([0, 0, 0], [4, 4, 4]), false, MaskMode::Set);
    assert_eq!(d.shown_selection().unwrap().len(), 64);
    assert!(d.cancel_box_select());
    assert_eq!(d.selection(), Some(&before));
    assert_eq!(history_len(&d), steps);
    // Undo with an open box only closes it.
    d.begin_box_select(area([0, 0, 0], [4, 4, 4]), false, MaskMode::Set);
    assert!(d.undo());
    assert!(d.box_select().is_none());
    assert_eq!(d.selection(), Some(&before));
}

#[test]
fn box_select_combines_by_the_mode_at_apply_time() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.select([IVec3::new(3, 3, 3)], MaskMode::Set);
    d.begin_box_select(area([0, 0, 0], [2, 1, 1]), false, MaskMode::Set);
    assert_eq!(d.shown_selection().unwrap().len(), 2);
    // Shift held when applying: added to the old selection.
    d.set_box_mode(MaskMode::Add);
    assert_eq!(d.shown_selection().unwrap().len(), 3);
    d.apply_box_select();
    assert_eq!(d.selection().unwrap().len(), 3);
    // Ctrl: subtracted.
    d.begin_box_select(area([0, 0, 0], [1, 1, 1]), false, MaskMode::Subtract);
    d.apply_box_select();
    let left: Selection = [IVec3::new(1, 0, 0), IVec3::new(3, 3, 3)]
        .into_iter()
        .collect();
    assert_eq!(d.selection(), Some(&left));
}

#[test]
fn reopened_box_replaces_its_own_result() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.select([IVec3::new(3, 3, 3)], MaskMode::Set);
    d.begin_box_select(area([0, 0, 0], [2, 1, 1]), false, MaskMode::Add);
    d.apply_box_select();
    assert_eq!(d.selection().unwrap().len(), 3);
    // Shrinking the re-opened box takes cells back out.
    assert!(d.reopen_last_box(MaskMode::Add));
    d.set_box_area(area([0, 0, 0], [1, 1, 1]), true);
    d.apply_box_select();
    assert_eq!(d.selection().unwrap().len(), 2);
}

#[test]
fn box_select_stays_on_the_slice() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.plane.axis = 1;
    d.plane.slice = true;
    let bounds = d.data.bounds;
    d.plane.set_slice_layer(2, bounds);
    d.begin_box_select(area([0, 0, 0], [4, 4, 4]), false, MaskMode::Set);
    let b = d.box_select().unwrap();
    assert_eq!((b.area.min.y, b.area.max.y), (2, 3), "one layer thick");
    assert_eq!(b.found(), 16);
}

#[test]
fn large_boxes_update_on_release() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.data.bounds = BoundSize::new(128, 128, 128);
    d.begin_box_select(area([0, 0, 0], [1, 1, 1]), false, MaskMode::Set);
    d.set_box_area(area([0, 0, 0], [128, 128, 128]), false);
    assert!(d.box_select().unwrap().stale);
    assert_eq!(d.box_select().unwrap().found(), 1);
    d.refresh_box_select();
    assert_eq!(d.box_select().unwrap().found(), 64);
}

// ------------------------------------------------------- to new layer

#[test]
fn cut_to_new_layer_moves_exactly_the_selection() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2), solid_layer("B", 3)]);
    d.active_layer = 0;
    let g = d.group_layer(0).unwrap();
    // The group wraps A: A is index 0, the group 1, B 2.
    d.active_layer = 0;
    let picked: Vec<IVec3> = (0..4).map(|x| IVec3::new(x, 1, 2)).collect();
    d.select(
        picked.iter().copied().chain([IVec3::splat(9)]),
        MaskMode::Set,
    );
    let before = d.data.clone();

    d.selection_to_new_layer(true).unwrap();
    assert_eq!(d.data.layers.len(), 4);
    assert_eq!(d.active_layer, 1, "right above the source");
    let new = &d.data.layers[1];
    assert_eq!(new.name, "A part");
    assert_eq!(new.props.parent, Some(g), "same group");
    assert_eq!(new.volume.voxel_count(), 4);
    let carried: Selection = picked.iter().copied().collect();
    assert_eq!(new.selection, carried, "only the solid selected cells");
    let src = &d.data.layers[0];
    assert_eq!(src.volume.voxel_count(), 60);
    assert!(picked.iter().all(|&p| src.volume.get(p) == 0));
    assert!(src.selection.is_empty());
    assert!(d.data.structure_is_valid());
    assert_eq!(d.history_entries().0.last().unwrap(), "To New Layer (Cut)");

    assert!(d.undo());
    assert_eq!(d.data, before, "one step restores both layers");
    assert_eq!(d.data.layers[0].selection.len(), 5);
}

#[test]
fn copy_to_new_layer_leaves_the_source() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.select([IVec3::ZERO, IVec3::ONE], MaskMode::Set);
    let source = d.data.layers[0].clone();
    d.selection_to_new_layer(false).unwrap();
    assert_eq!(d.data.layers[0], source);
    assert_eq!(d.data.layers[1].volume.voxel_count(), 2);
    assert_eq!(d.data.layers[1].selection.len(), 2);
    // A second copy gets a unique name.
    d.active_layer = 0;
    d.selection_to_new_layer(false).unwrap();
    assert_eq!(d.data.layers[1].name, "A part 2");
    // Nothing selected, or Mask Mode on: refused, nothing recorded.
    let steps = history_len(&d);
    d.active_layer = 0;
    d.mask_selection = true;
    assert!(d.selection_to_new_layer(true).is_err());
    d.mask_selection = false;
    d.clear_selection();
    let steps_after_clear = history_len(&d);
    assert!(steps_after_clear > steps);
    assert!(d.selection_to_new_layer(false).is_err());
    assert_eq!(history_len(&d), steps_after_clear);
}

#[test]
fn clone_and_shape_sources_are_baked_on_cut() {
    use rizxel_math::{LayerKind, ShapeKind};
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    let shape = area([10, 0, 0], [12, 2, 2]);
    d.add_shape_layer(ShapeKind::Cube, shape, 7);
    let s = d.active_layer;
    d.data.layers[s].selection = [IVec3::new(10, 0, 0), IVec3::new(0, 0, 0)]
        .into_iter()
        .collect();
    d.selection_to_new_layer(true).unwrap();
    assert!(matches!(d.data.layers[s].kind(), LayerKind::Voxels));
    assert_eq!(d.data.layers[s].volume.voxel_count(), 7);
    assert_eq!(d.data.layers[s + 1].volume.get(IVec3::new(10, 0, 0)), 7);
    assert_eq!(d.data.layers[s + 1].volume.voxel_count(), 1);
    d.undo();
    assert!(matches!(d.data.layers[s].kind(), LayerKind::Shape { .. }));

    // A linked clone: copied from its moved geometry.
    d.active_layer = 0;
    d.add_linked_clone(0, IVec3::new(0, 0, 5)).unwrap();
    let c = d.active_layer;
    d.data.layers[c].selection = [IVec3::new(0, 0, 5)].into_iter().collect();
    d.selection_to_new_layer(false).unwrap();
    assert_eq!(d.data.layers[c + 1].volume.get(IVec3::new(0, 0, 5)), 2);
    assert!(matches!(d.data.layers[c].kind(), LayerKind::Clone { .. }));
}
