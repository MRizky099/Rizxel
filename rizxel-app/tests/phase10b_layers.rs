//! Phase 10b: the layer hierarchy (grouping, ungrouping, moving subtrees,
//! dropping rows, inherited lock, group duplicate / merge / delete).

use glam::IVec3;
use rizxel_app::{Document, EditBlock, Layer, LayerDrop, VoxelChange};
use rizxel_io::ProjectFile;
use rizxel_math::{BoundSize, LayerId, LayerKind};

fn layer(name: &str, x: i32) -> Layer {
    let mut l = Layer::new(name, BoundSize::default());
    l.volume.set(IVec3::new(x, 0, 0), 2 + x as u8);
    l
}

/// Layers A, B, C, D bottom to top, one voxel each at x = index.
fn doc() -> Document {
    Document::with_layers(
        "t",
        ["A", "B", "C", "D"]
            .iter()
            .enumerate()
            .map(|(i, n)| layer(n, i as i32))
            .collect(),
    )
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

/// A, [B, C] G, D: B and C in group G.
fn grouped() -> Document {
    let mut d = doc();
    d.active_layer = 1;
    let g = d.group_layer(1).unwrap();
    // C joins G by dropping it into the group row.
    assert!(d.drop_layer(index_of(&d, "C"), LayerDrop::Into(index_of(&d, "Group"))));
    assert_eq!(names(&d), ["A", "B", "C", "Group", "D"]);
    assert_eq!(d.data.layers[3].id, g);
    d
}

#[test]
fn group_and_ungroup_are_single_steps() {
    let mut d = doc();
    let before = d.data.clone();
    let g = d.group_layer(2).unwrap();
    assert_eq!(names(&d), ["A", "B", "C", "Group", "D"]);
    assert_eq!(d.active_layer, 3);
    assert_eq!(parent_name(&d, "C").as_deref(), Some("Group"));
    assert!(d.data.structure_is_valid());
    assert!(d.undo());
    assert_eq!(d.data, before);
    assert!(d.redo());
    let grouped = d.data.clone();

    assert!(d.ungroup(d.data.layer_index(g).unwrap()));
    assert_eq!(names(&d), ["A", "B", "C", "D"]);
    assert_eq!(parent_name(&d, "C"), None);
    assert_eq!(d.active_layer, 2, "the top child becomes active");
    assert!(d.undo());
    assert_eq!(d.data, grouped);
    assert_eq!(d.history_entries().0, ["Group Layer", "Ungroup"]);
    // Only groups can be dissolved.
    assert!(!d.ungroup(0));
}

#[test]
fn new_layers_go_inside_the_active_group() {
    let mut d = grouped();
    d.active_layer = index_of(&d, "Group");
    d.add_layer();
    let new = d.data.layers[d.active_layer].name.clone();
    assert_eq!(parent_name(&d, &new).as_deref(), Some("Group"));
    assert_eq!(d.active_layer + 1, index_of(&d, "Group"), "top child");
    // A nested group, then a layer inside it.
    d.add_group();
    d.add_layer();
    let inner = d.data.layers[d.active_layer].name.clone();
    assert_eq!(parent_name(&d, &inner).as_deref(), Some("Group 2"));
    assert_eq!(parent_name(&d, "Group 2").as_deref(), Some("Group"));
    assert_eq!(d.data.tree().depth(d.active_layer), 2);
    assert!(d.data.structure_is_valid());
}

#[test]
fn dragging_a_group_moves_its_subtree() {
    let mut d = grouped();
    let before = d.data.clone();
    // Drop G (with B and C) above D.
    assert!(d.drop_layer(index_of(&d, "Group"), LayerDrop::Above(index_of(&d, "D"))));
    assert_eq!(names(&d), ["A", "D", "B", "C", "Group"]);
    assert_eq!(parent_name(&d, "B").as_deref(), Some("Group"));
    assert!(d.undo());
    assert_eq!(d.data, before);
    // Below A: the bottom of the list.
    assert!(d.drop_layer(index_of(&d, "Group"), LayerDrop::Below(0)));
    assert_eq!(names(&d), ["B", "C", "Group", "A", "D"]);
    assert_eq!(d.history_entries().0.len(), 3, "one step per drop");
}

#[test]
fn dragging_out_of_and_between_groups() {
    let mut d = grouped();
    // C leaves the group: dropped above the group row.
    assert!(d.drop_layer(index_of(&d, "C"), LayerDrop::Above(index_of(&d, "Group"))));
    assert_eq!(names(&d), ["A", "B", "Group", "C", "D"]);
    assert_eq!(parent_name(&d, "C"), None);
    // D goes into the group below B (same group as B).
    assert!(d.drop_layer(index_of(&d, "D"), LayerDrop::Below(index_of(&d, "B"))));
    assert_eq!(names(&d), ["A", "D", "B", "Group", "C"]);
    assert_eq!(parent_name(&d, "D").as_deref(), Some("Group"));
    assert!(d.data.structure_is_valid());
}

#[test]
fn drops_that_would_break_the_tree_are_refused() {
    let mut d = grouped();
    let g = index_of(&d, "Group");
    d.active_layer = index_of(&d, "B");
    let inner = d.group_layer(index_of(&d, "B")).unwrap();
    let inner = d.data.layer_index(inner).unwrap();
    let g = g + 1;
    assert_eq!(d.data.layers[g].name, "Group");
    let before = d.data.clone();
    // A group into its own child group, onto itself, into a non-group.
    assert_eq!(d.drop_destination(g, LayerDrop::Into(inner)), None);
    assert!(!d.drop_layer(g, LayerDrop::Into(inner)));
    assert!(!d.drop_layer(g, LayerDrop::Above(g)));
    assert!(!d.drop_layer(0, LayerDrop::Into(index_of(&d, "D"))));
    // A drop that changes nothing records nothing.
    assert!(!d.drop_layer(index_of(&d, "C"), LayerDrop::Above(index_of(&d, "Group 2"))));
    assert_eq!(d.data, before);
}

#[test]
fn locked_group_blocks_tools_on_its_children() {
    let mut d = grouped();
    let (b, g) = (index_of(&d, "B"), index_of(&d, "Group"));
    d.active_layer = b;
    assert_eq!(d.editable_layer(b), Ok(()));
    d.toggle_layer_locked(g);
    assert_eq!(d.editable_layer(b), Err(EditBlock::LockedGroup));
    assert!(d.data.effectively_locked(b));
    let brush = [VoxelChange {
        pos: IVec3::new(5, 5, 5),
        old: 0,
        new: 4,
    }];
    assert_eq!(d.edit_voxels(b, &brush), 0, "Brush is refused");
    // Layers outside the group are unaffected.
    assert_eq!(d.editable_layer(0), Ok(()));
    d.toggle_layer_locked(g);
    assert_eq!(d.edit_voxels(b, &brush), 1);
    // A group itself has no voxels: the message asks for a voxel layer.
    assert!(EditBlock::NotVoxels
        .message()
        .contains("select a voxel layer"));
    assert_eq!(d.editable_layer(g), Err(EditBlock::NotVoxels));
}

#[test]
fn move_up_and_down_step_over_sibling_subtrees() {
    let mut d = grouped();
    d.active_layer = index_of(&d, "A");
    assert!(d.move_active_layer(true));
    assert_eq!(names(&d), ["B", "C", "Group", "A", "D"]);
    assert_eq!(d.active_layer, 3, "the active layer follows");
    assert!(d.move_active_layer(false));
    assert_eq!(names(&d), ["A", "B", "C", "Group", "D"]);
    // Inside the group: C is the top child and cannot leave by stepping.
    d.active_layer = index_of(&d, "C");
    assert!(!d.move_active_layer(true));
    assert!(d.move_active_layer(false));
    assert_eq!(names(&d), ["A", "C", "B", "Group", "D"]);
    assert!(!d.move_active_layer(false), "C is now the bottom child");
}

#[test]
fn duplicating_a_group_copies_its_subtree() {
    let mut d = grouped();
    let b = index_of(&d, "B");
    d.active_layer = b;
    let clone = d.add_linked_clone(b, IVec3::new(0, 4, 0)).unwrap();
    let g = index_of(&d, "Group");
    d.active_layer = g;
    let before = d.data.clone();
    assert!(d.duplicate_active_layer());
    assert_eq!(
        names(&d),
        [
            "A",
            "B",
            "B (linked)",
            "C",
            "Group",
            "B",
            "B (linked)",
            "C",
            "Group copy",
            "D"
        ]
    );
    assert_eq!(d.active_layer, index_of(&d, "Group copy"));
    // The copied clone points at the copied B, the original at the original.
    let copy_b = d.data.layers[5].id;
    assert!(matches!(d.data.layers[6].props.kind,
        LayerKind::Clone { source, .. } if source == copy_b));
    assert_eq!(d.data.layers[2].id, clone);
    assert_eq!(parent_name(&d, "Group copy"), None);
    assert_eq!(
        d.data.layers[5].props.parent,
        Some(d.data.layers[8].id),
        "children join the copy"
    );
    let ids: std::collections::HashSet<LayerId> = d.data.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids.len(), d.data.layers.len());
    assert!(d.undo());
    assert_eq!(d.data, before);
}

#[test]
fn merge_and_delete_group() {
    let mut d = grouped();
    let g = index_of(&d, "Group");
    // Hidden children are dropped from the merge.
    d.toggle_layer_visible(index_of(&d, "C"));
    let before = d.data.clone();
    assert!(d.merge_group(g));
    assert_eq!(names(&d), ["A", "Group", "D"]);
    let merged = &d.data.layers[1];
    assert!(merged.kind().stores_voxels());
    assert_eq!(merged.volume.voxel_count(), 1, "B only");
    assert_eq!(merged.volume.get(IVec3::new(1, 0, 0)), 3);
    assert!(d.undo());
    assert_eq!(d.data, before);

    assert!(d.delete_group_with_contents(g));
    assert_eq!(names(&d), ["A", "D"]);
    assert!(d.undo());
    assert_eq!(d.data, before);
    // Refused while a clone outside points inside.
    d.active_layer = index_of(&d, "D");
    d.add_linked_clone(index_of(&d, "B"), IVec3::ZERO).unwrap();
    assert!(!d.can_delete_group_with_contents(g));
    assert!(!d.delete_group_with_contents(g));
    assert!(!d.merge_group(g));
}

#[test]
fn collapse_is_view_state() {
    let mut d = grouped();
    let g = index_of(&d, "Group");
    let steps = d.history_entries().0.len();
    assert!(d.set_group_collapsed(g, true));
    assert!(!d.set_group_collapsed(g, true));
    assert!(!d.set_group_collapsed(0, true), "not a group");
    assert_eq!(d.history_entries().0.len(), steps);
    assert_eq!(
        d.data.layers[g].props.kind,
        LayerKind::Group { collapsed: true }
    );
    // Saved with the file.
    let back = Document::from_project(d.to_project(), None).unwrap();
    assert_eq!(
        back.data.layers[g].props.kind,
        LayerKind::Group { collapsed: true }
    );
}

#[test]
fn files_with_groups_first_are_reordered_on_load() {
    // Goxel order: the group row first, children after it.
    let mut p: ProjectFile = doc().to_project();
    p.layers.insert(
        0,
        rizxel_io::LayerData {
            kind: LayerKind::Group { collapsed: false },
            ..rizxel_io::LayerData::new(99, "G")
        },
    );
    p.layers[2].parent = Some(99);
    p.layers[4].parent = Some(99);
    p.active_layer = 4;
    let d = Document::from_project(p, None).unwrap();
    assert_eq!(names(&d), ["B", "D", "G", "A", "C"]);
    assert!(d.data.structure_is_valid());
    assert_eq!(d.data.layers[d.active_layer].name, "D", "active follows");
    assert!(!d.is_dirty());
}

/// Names and group names, bottom to top, ignoring the order of siblings'
/// groups within the list.
fn tree_shape(d: &Document) -> Vec<(String, Option<String>, bool)> {
    let mut v: Vec<_> = d
        .data
        .layers
        .iter()
        .map(|l| (l.name.clone(), parent_name(d, &l.name), l.kind().is_group()))
        .collect();
    v.sort();
    v
}

#[test]
fn nested_groups_survive_vox_and_gox() {
    // A, [B, [C] Inner] Outer, D
    let mut d = grouped();
    d.rename_layer(index_of(&d, "Group"), "Outer".into());
    d.group_layer(index_of(&d, "C")).unwrap();
    d.rename_layer(index_of(&d, "Group"), "Inner".into());
    assert_eq!(names(&d), ["A", "B", "C", "Inner", "Outer", "D"]);

    let vox = rizxel_io::write_vox(&d.export_scene()).unwrap();
    let back = Document::from_project(rizxel_io::read_vox(&vox, "t").unwrap(), None).unwrap();
    assert_eq!(tree_shape(&back), tree_shape(&d));
    assert_eq!(names(&back), names(&d), "draw order is kept");

    let gox = rizxel_io::write_gox(&d.to_project()).unwrap();
    let back = Document::from_project(rizxel_io::read_gox(&gox, "t").unwrap(), None).unwrap();
    assert_eq!(tree_shape(&back), tree_shape(&d));
    assert!(back.data.structure_is_valid());
}
