//! Phase 12c: multi-select in the Layers panel (click, `Ctrl`+click,
//! `Shift`+click ranges over collapsed groups), *Group Selected*, dragging
//! a selection, the bulk operations, and *select on click*.

use glam::IVec3;
use rizxel_app::layer_select::SELECT_LAYER_LABEL;
use rizxel_app::{Document, Layer, LayerDrop};
use rizxel_math::{BoundSize, LayerId, LayerKind};

fn layer(id: LayerId, name: &str, parent: Option<LayerId>, x: Option<i32>) -> Layer {
    let mut l = Layer::new(name, BoundSize::default());
    l.id = id;
    l.props.parent = parent;
    if let Some(x) = x {
        l.volume.set(IVec3::new(x, 0, 0), 2);
        l.volume.set(IVec3::new(x, 1, 0), 3);
    }
    l
}

fn group(id: LayerId, name: &str) -> Layer {
    let mut l = layer(id, name, None, None);
    l.props.kind = LayerKind::Group { collapsed: false };
    l
}

/// Bottom to top: A, B, [C, D] G1, E, [F] G2, H. Panel rows, top first:
/// H, G2, F, E, G1, D, C, B, A.
fn doc() -> Document {
    let d = Document::with_layers(
        "t",
        vec![
            layer(1, "A", None, Some(0)),
            layer(2, "B", None, Some(1)),
            layer(3, "C", Some(5), Some(2)),
            layer(4, "D", Some(5), Some(3)),
            group(5, "G1"),
            layer(6, "E", None, Some(4)),
            layer(7, "F", Some(8), Some(5)),
            group(8, "G2"),
            layer(9, "H", None, Some(6)),
        ],
    );
    assert!(d.data.structure_is_valid());
    d
}

fn names(d: &Document) -> Vec<String> {
    d.data.layers.iter().map(|l| l.name.clone()).collect()
}

fn index_of(d: &Document, name: &str) -> usize {
    d.data.layers.iter().position(|l| l.name == name).unwrap()
}

fn parent_name(d: &Document, name: &str) -> Option<String> {
    let p = d.data.layers[index_of(d, name)].props.parent?;
    d.data.layer_index(p).map(|i| d.data.layers[i].name.clone())
}

fn selected_names(d: &Document) -> Vec<String> {
    d.selected_layer_indices()
        .into_iter()
        .map(|i| d.data.layers[i].name.clone())
        .collect()
}

/// Selects the layers `list` with a plain click on the first and
/// `Ctrl`+clicks on the rest.
fn select(d: &mut Document, list: &[&str]) {
    d.select_layer(index_of(d, list[0]));
    for name in &list[1..] {
        d.toggle_layer_selected(index_of(d, name));
    }
}

fn undo_steps(d: &Document) -> usize {
    d.history_entries().1
}

#[test]
fn click_and_ctrl_click_keep_the_active_layer_selected() {
    let mut d = doc();
    d.select_layer(index_of(&d, "A"));
    assert_eq!(selected_names(&d), ["A"]);
    d.toggle_layer_selected(index_of(&d, "E"));
    assert_eq!(selected_names(&d), ["A", "E"]);
    assert_eq!(d.active_layer, index_of(&d, "E"));
    // Removing the active one: the last one picked before becomes active.
    d.toggle_layer_selected(index_of(&d, "E"));
    assert_eq!(selected_names(&d), ["A"]);
    assert_eq!(d.active_layer, index_of(&d, "A"));
    // The last selected layer stays.
    d.toggle_layer_selected(index_of(&d, "A"));
    assert_eq!(selected_names(&d), ["A"]);
    // Making another layer active elsewhere (a new layer) resets it.
    select(&mut d, &["A", "B"]);
    d.active_layer = index_of(&d, "H");
    assert_eq!(selected_names(&d), ["H"]);
}

#[test]
fn shift_click_selects_rows_counting_a_collapsed_group_as_one() {
    let mut d = doc();
    d.set_group_collapsed(index_of(&d, "G1"), true);
    d.select_layer(index_of(&d, "B"));
    d.select_layer_range(index_of(&d, "F"), false);
    // Rows B, G1 (folded: C and D are not rows), E, F.
    assert_eq!(selected_names(&d), ["B", "G1", "E", "F"]);
    assert_eq!(d.active_layer, index_of(&d, "F"));
    // The anchor stays on B: a new range replaces this one.
    d.select_layer_range(index_of(&d, "A"), false);
    assert_eq!(selected_names(&d), ["A", "B"]);
    // Ctrl+Shift adds a range; the anchor moves with Ctrl+click.
    d.toggle_layer_selected(index_of(&d, "H"));
    d.select_layer_range(index_of(&d, "G2"), true);
    assert_eq!(selected_names(&d), ["A", "B", "G2", "H"]);
    // A collapsed group's child as the target counts as the group's row.
    d.select_layer(index_of(&d, "E"));
    d.select_layer_range(index_of(&d, "C"), false);
    assert_eq!(selected_names(&d), ["G1", "E"]);
    // Select all, and Esc back to the active layer.
    d.select_all_layers();
    assert_eq!(d.selected_layer_count(), 9);
    d.clear_layer_selection();
    assert_eq!(d.selected_layer_count(), 1);
}

#[test]
fn group_selected_gathers_a_mixed_selection_in_one_step() {
    let mut d = doc();
    // A (top level), C (in G1), and G2 with its child F also selected.
    select(&mut d, &["A", "C", "F", "G2"]);
    assert_eq!(
        d.normalized_layer_selection(),
        [0, 2, 7],
        "F rides along with G2"
    );
    let before = d.data.clone();
    let steps = undo_steps(&d);
    let id = d.group_selected_layers().unwrap();
    // The group sits where G2 (the top-most selected) was, at the top
    // level (the deepest group holding all of them), keeping the order.
    assert_eq!(
        names(&d),
        ["B", "D", "G1", "E", "A", "C", "F", "G2", "Group", "H"]
    );
    for child in ["A", "C", "G2"] {
        assert_eq!(parent_name(&d, child).as_deref(), Some("Group"), "{child}");
    }
    assert_eq!(parent_name(&d, "F").as_deref(), Some("G2"));
    assert_eq!(parent_name(&d, "D").as_deref(), Some("G1"));
    assert_eq!(parent_name(&d, "Group"), None);
    assert!(d.data.structure_is_valid());
    assert_eq!(undo_steps(&d), steps + 1);
    assert_eq!(d.history_entries().0.last().unwrap(), "Group Layers");
    // The new group is active, selected alone and expanded.
    assert_eq!(d.data.layers[d.active_layer].id, id);
    assert_eq!(selected_names(&d), ["Group"]);
    assert_eq!(
        d.data.layers[d.active_layer].props.kind,
        LayerKind::Group { collapsed: false }
    );
    assert!(d.undo());
    assert_eq!(d.data, before);
    assert!(d.redo());
    assert_eq!(names(&d)[8], "Group");
}

#[test]
fn group_selected_inside_a_group_stays_in_it() {
    let mut d = doc();
    select(&mut d, &["C", "D"]);
    d.group_selected_layers().unwrap();
    assert_eq!(
        names(&d),
        ["A", "B", "C", "D", "Group", "G1", "E", "F", "G2", "H"]
    );
    assert_eq!(parent_name(&d, "Group").as_deref(), Some("G1"));
    assert_eq!(parent_name(&d, "C").as_deref(), Some("Group"));
    // One layer selected: Group Layer, as before.
    d.select_layer(index_of(&d, "H"));
    d.group_selected_layers().unwrap();
    assert_eq!(parent_name(&d, "H").as_deref(), Some("Group 2"));
}

#[test]
fn dragging_a_selection_keeps_its_order_in_one_step() {
    let mut d = doc();
    select(&mut d, &["A", "E", "H"]);
    let before = d.data.clone();
    let steps = undo_steps(&d);
    assert!(d.drags_selection(index_of(&d, "E")));
    assert!(!d.drags_selection(index_of(&d, "B")));
    let g2 = index_of(&d, "G2");
    assert!(d.can_drop_selection(LayerDrop::Into(g2)));
    assert!(d.drop_selected_layers(LayerDrop::Into(g2)));
    assert_eq!(names(&d), ["B", "C", "D", "G1", "F", "A", "E", "H", "G2"]);
    for child in ["A", "E", "H", "F"] {
        assert_eq!(parent_name(&d, child).as_deref(), Some("G2"), "{child}");
    }
    assert_eq!(undo_steps(&d), steps + 1);
    assert_eq!(d.history_entries().0.last().unwrap(), "Move Layers");
    assert_eq!(selected_names(&d), ["A", "E", "H"], "the selection stays");
    assert!(d.undo());
    assert_eq!(d.data, before);

    // Above and below a row, out of a group to the top level.
    select(&mut d, &["C", "F"]);
    assert!(d.drop_selected_layers(LayerDrop::Above(index_of(&d, "H"))));
    assert_eq!(names(&d), ["A", "B", "D", "G1", "E", "G2", "H", "C", "F"]);
    assert_eq!(parent_name(&d, "C"), None);
    assert!(d.undo());
    select(&mut d, &["C", "F"]);
    assert!(d.drop_selected_layers(LayerDrop::Below(index_of(&d, "A"))));
    assert_eq!(names(&d), ["C", "F", "A", "B", "D", "G1", "E", "G2", "H"]);
}

#[test]
fn dropping_into_a_selected_subtree_is_refused() {
    let mut d = doc();
    select(&mut d, &["B", "G1"]);
    let before = d.data.clone();
    for target in [
        LayerDrop::Into(index_of(&d, "G1")),
        LayerDrop::Above(index_of(&d, "C")),
        LayerDrop::Below(index_of(&d, "D")),
    ] {
        assert!(!d.can_drop_selection(target), "{target:?}");
        assert!(!d.drop_selected_layers(target));
    }
    assert_eq!(d.data, before);
    // Elsewhere it moves, G1 with its children.
    assert!(d.drop_selected_layers(LayerDrop::Above(index_of(&d, "H"))));
    assert_eq!(names(&d), ["A", "E", "F", "G2", "H", "B", "C", "D", "G1"]);
    assert_eq!(parent_name(&d, "C").as_deref(), Some("G1"));
}

#[test]
fn bulk_delete_duplicate_show_lock_and_merge() {
    let mut d = doc();
    // Delete: G1 goes, its unselected children move up a level.
    select(&mut d, &["B", "G1"]);
    let before = d.data.clone();
    assert!(d.delete_selected_layers());
    assert_eq!(names(&d), ["A", "C", "D", "E", "F", "G2", "H"]);
    assert_eq!(parent_name(&d, "C"), None);
    assert_eq!(d.history_entries().0.last().unwrap(), "Delete Layers");
    assert_eq!(d.active_layer, index_of(&d, "A"));
    assert!(d.undo());
    assert_eq!(d.data, before);
    // Everything selected can't be deleted.
    d.select_all_layers();
    assert!(!d.can_delete_selected_layers());

    // Duplicate: each copy above its layer, the copies selected.
    select(&mut d, &["A", "E"]);
    let steps = undo_steps(&d);
    assert!(d.duplicate_selected_layers());
    assert_eq!(undo_steps(&d), steps + 1);
    assert_eq!(index_of(&d, "A copy"), index_of(&d, "A") + 1);
    assert_eq!(index_of(&d, "E copy"), index_of(&d, "E") + 1);
    assert_eq!(selected_names(&d), ["A copy", "E copy"]);
    assert_eq!(d.data.layers[d.active_layer].name, "E copy");
    assert!(d.undo());

    // Visibility and lock: one step each, a selected group carries its own.
    select(&mut d, &["A", "G2", "F"]);
    let steps = undo_steps(&d);
    assert!(d.set_selected_layers_visible(false));
    assert!(d.set_selected_layers_locked(true));
    assert_eq!(undo_steps(&d), steps + 2);
    let flags = |d: &Document, n: &str| {
        let l = &d.data.layers[index_of(d, n)];
        (l.visible, l.locked)
    };
    assert_eq!(flags(&d, "A"), (false, true));
    assert_eq!(flags(&d, "G2"), (false, true));
    assert_eq!(flags(&d, "F"), (true, false), "inherits from G2 instead");
    d.undo();
    d.undo();

    // Merge into the lowest selected voxel layer; groups refuse.
    select(&mut d, &["A", "B", "E"]);
    assert!(d.can_merge_selected_layers());
    assert!(d.merge_selected_layers());
    assert_eq!(names(&d), ["A", "C", "D", "G1", "F", "G2", "H"]);
    let a = &d.data.layers[0].volume;
    assert_eq!(a.voxel_count(), 6);
    assert_ne!(a.get(IVec3::new(4, 0, 0)), 0, "E's voxels are in A");
    assert_eq!(d.history_entries().0.last().unwrap(), "Merge Layers");
    d.undo();
    select(&mut d, &["A", "G1"]);
    assert!(!d.can_merge_selected_layers());
}

#[test]
fn select_on_click_fills_layer_selections_as_one_step() {
    let mut d = doc();
    let steps = undo_steps(&d);
    // Clicking through three layers, one of them twice.
    for name in ["A", "B", "E", "A"] {
        let i = index_of(&d, name);
        d.select_layer(i);
        d.select_layer_voxels(&[i]);
    }
    for name in ["A", "B", "E"] {
        assert_eq!(
            d.data.layers[index_of(&d, name)].selection.len(),
            2,
            "{name}"
        );
    }
    assert_eq!(undo_steps(&d), steps + 1, "one step for every click");
    assert_eq!(d.history_entries().0.last().unwrap(), SELECT_LAYER_LABEL);
    // Groups and empty layers select nothing.
    assert!(!d.select_layer_voxels(&[index_of(&d, "G1")]));
    // Delete then empties the layer.
    d.select_layer(index_of(&d, "B"));
    assert_eq!(d.delete_selected(), 2);
    assert_eq!(d.data.layers[index_of(&d, "B")].volume.voxel_count(), 0);
    d.undo();
    // Undo goes back to before the first click.
    assert!(d.undo());
    assert!(d.data.layers.iter().all(|l| l.selection.is_empty()));
    assert_eq!(undo_steps(&d), steps);
}

#[test]
fn the_layer_selection_is_not_saved() {
    let mut d = doc();
    let plain = d.to_project();
    d.select_all_layers();
    assert_eq!(d.to_project(), plain, "no change to .rzxl");
    let reopened = Document::from_project(d.to_project(), None).unwrap();
    assert_eq!(reopened.selected_layer_count(), 1);
}
