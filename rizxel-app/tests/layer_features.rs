//! `.rzxl` version 3 layer features through the document: groups, linked
//! clones, shape layers, crop boxes, opacity and materials, including undo
//! and save/load round trips.

use glam::IVec3;
use rizxel_app::{AppState, Document, LayerProps};
use rizxel_math::{BoundSize, LayerKind, Material, ShapeKind, VoxelBox};

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rizxel_{name}_{}", std::process::id()))
}

/// 2×2×2 block of color 9 in layer 0 of a 16³ document.
fn doc_with_block() -> Document {
    let mut d = Document::new("t");
    d.set_bounds(BoundSize::new(16, 16, 16), false);
    let block: Vec<_> = (0..8)
        .map(|i| (IVec3::new(i & 1, (i >> 1) & 1, (i >> 2) & 1), 9))
        .collect();
    assert!(d.set_voxels(0, block, false));
    d
}

fn props(d: &Document, index: usize) -> LayerProps {
    d.data.layers[index].props.clone()
}

#[test]
fn linked_clone_follows_its_source() {
    let mut d = doc_with_block();
    let clone_id = d.add_linked_clone(0, IVec3::new(4, 0, 0)).unwrap();
    assert_eq!(d.visible_voxel_count(), 16);
    assert_eq!(d.data.layers[1].id, clone_id);

    // Editing the source updates the clone too.
    d.set_voxels(0, [(IVec3::new(0, 5, 0), 3)], false);
    assert_eq!(d.visible_voxel_count(), 18);
    let top = d.occupied_bounds().unwrap();
    assert_eq!(top, (IVec3::ZERO, IVec3::new(6, 6, 2)));

    // Clones store no voxels and cannot be painted.
    assert!(!d.set_voxels(1, [(IVec3::splat(9), 1)], false));
    // The source cannot be deleted while linked.
    d.active_layer = 0;
    assert!(!d.can_delete_active_layer());
    assert!(!d.delete_active_layer());
    // Hiding the source keeps the clone visible.
    d.toggle_layer_visible(0);
    assert_eq!(d.visible_voxel_count(), 9);
}

#[test]
fn hidden_group_hides_children_and_delete_reparents() {
    let mut d = doc_with_block();
    let group = d.add_group();
    let gi = d.active_layer;
    // Move layer 0 into the group.
    let p = LayerProps {
        parent: Some(group),
        ..props(&d, 0)
    };
    assert!(d.set_layer_props(0, p, false));
    assert_eq!(d.visible_voxel_count(), 8);
    d.toggle_layer_visible(gi);
    assert_eq!(d.visible_voxel_count(), 0, "hidden group hides its child");
    d.toggle_layer_visible(gi);

    // A group cannot be its own parent, and non-groups cannot be parents.
    let selfish = LayerProps {
        parent: Some(group),
        ..props(&d, gi)
    };
    assert!(!d.set_layer_props(gi, selfish, false));
    let bad_parent = LayerProps {
        parent: Some(d.data.layers[0].id),
        ..props(&d, gi)
    };
    assert!(!d.set_layer_props(gi, bad_parent, false));

    // Deleting the group moves the child to the top level in one undo step.
    d.active_layer = gi;
    assert!(d.delete_active_layer());
    assert_eq!(d.data.layers.len(), 1);
    assert_eq!(d.data.layers[0].props.parent, None);
    assert!(d.undo());
    assert_eq!(d.data.layers.len(), 2);
    assert_eq!(d.data.layers[0].props.parent, Some(group));
}

#[test]
fn shape_layer_and_crop_box() {
    let mut d = Document::new("t");
    d.set_bounds(BoundSize::new(16, 16, 16), false);
    let area = VoxelBox::from_corners(IVec3::ZERO, IVec3::new(3, 3, 3));
    d.add_shape_layer(ShapeKind::Cube, area, 12);
    let si = d.active_layer;
    assert_eq!(d.visible_voxel_count(), 64);
    assert!(!d.set_voxels(si, [(IVec3::splat(8), 1)], false));

    // Crop to the lower half (non-destructive).
    let crop = VoxelBox::from_corners(IVec3::ZERO, IVec3::new(15, 1, 15));
    let p = LayerProps {
        crop: Some(crop),
        ..props(&d, si)
    };
    assert!(d.set_layer_props(si, p, false));
    assert_eq!(d.visible_voxel_count(), 32);
    assert!(d.undo());
    assert_eq!(d.visible_voxel_count(), 64);

    // Shape color must be a paint color.
    let p = LayerProps {
        kind: LayerKind::Shape {
            shape: ShapeKind::Sphere,
            area,
            color: 0,
        },
        ..props(&d, si)
    };
    assert!(!d.set_layer_props(si, p, false));
}

#[test]
fn converting_a_voxel_layer_is_undoable() {
    let mut d = doc_with_block();
    let p = LayerProps {
        kind: LayerKind::Group { collapsed: false },
        ..props(&d, 0)
    };
    assert!(d.set_layer_props(0, p, false));
    assert!(d.data.layers[0].volume.is_empty());
    assert_eq!(d.visible_voxel_count(), 0);
    assert!(d.undo());
    assert_eq!(d.data.layers[0].volume.voxel_count(), 8);
}

#[test]
fn opacity_drags_merge_and_materials_are_checked() {
    let mut d = doc_with_block();
    for o in [0.8, 0.5, 0.25] {
        let p = LayerProps {
            opacity: o,
            ..props(&d, 0)
        };
        assert!(d.set_layer_props(0, p, true));
    }
    assert_eq!(d.data.layers[0].props.opacity, 0.25);
    assert!(d.undo());
    assert_eq!(d.data.layers[0].props.opacity, 1.0, "one undo step");

    // Out-of-range opacity and unknown materials are refused.
    let bad = LayerProps {
        opacity: 2.0,
        ..props(&d, 0)
    };
    assert!(!d.set_layer_props(0, bad, false));
    let no_material = LayerProps {
        material: Some(0),
        ..props(&d, 0)
    };
    assert!(!d.set_layer_props(0, no_material.clone(), false));

    assert!(d.set_materials(vec![Material::new("Matte")]));
    assert!(d.set_layer_props(0, no_material, false));
    // Shrinking the list resets the reference in the same step.
    assert!(d.set_materials(Vec::new()));
    assert_eq!(d.data.layers[0].props.material, None);
    assert!(d.undo());
    assert_eq!(d.data.layers[0].props.material, Some(0));
    assert_eq!(d.data.materials.len(), 1);
}

#[test]
fn merge_visible_bakes_shapes_and_clones() {
    let mut d = doc_with_block();
    d.add_linked_clone(0, IVec3::new(4, 0, 0)).unwrap();
    let area = VoxelBox::from_corners(IVec3::new(8, 0, 0), IVec3::new(9, 0, 0));
    d.add_shape_layer(ShapeKind::Cube, area, 5);
    let before = d.visible_voxel_count();
    assert!(d.merge_visible_layers());
    assert_eq!(d.data.layers.len(), 1);
    assert_eq!(d.data.layers[0].kind(), &LayerKind::Voxels);
    assert_eq!(d.visible_voxel_count(), before);
    assert!(d.undo());
    assert_eq!(d.data.layers.len(), 3);
}

#[test]
fn all_features_survive_save_and_load() {
    let mut state = AppState::default();
    let d = state.doc_mut().unwrap();
    *d = doc_with_block();
    d.set_materials(vec![
        Material::new("Matte"),
        Material {
            emission: [0.2, 0.8, 1.0],
            ..Material::new("Neon")
        },
    ]);
    let group = d.add_group();
    d.active_layer = 0;
    d.add_linked_clone(0, IVec3::new(0, 0, 5)).unwrap();
    let ci = d.active_layer;
    let p = LayerProps {
        opacity: 0.5,
        parent: Some(group),
        material: Some(1),
        crop: Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::new(15, 0, 15))),
        ..props(d, ci)
    };
    assert!(d.set_layer_props(ci, p, false));
    d.add_shape_layer(
        ShapeKind::Cylinder,
        VoxelBox::from_corners(IVec3::new(8, 0, 8), IVec3::new(13, 6, 13)),
        20,
    );
    let count = d.visible_voxel_count();

    let path = temp_path("v3_layers.rzxl");
    state.save_active_as(&path).unwrap();
    let mut reopened = AppState::default();
    reopened.open(&path).unwrap();
    std::fs::remove_file(&path).ok();

    let (a, b) = (state.doc().unwrap(), reopened.doc().unwrap());
    assert_eq!(a.data, b.data);
    assert_eq!(b.visible_voxel_count(), count);
    assert_eq!(b.visible_mesh(), a.visible_mesh());
    // New layers keep getting fresh ids after a reload.
    let mut b = Document::from_project(b.to_project(), None).unwrap();
    let ids: Vec<_> = b.data.layers.iter().map(|l| l.id).collect();
    let new_id = b.add_layer();
    assert!(!ids.contains(&new_id));
}

#[test]
fn version_2_files_open_as_plain_voxel_layers() {
    // Write a v3 file, then rewrite its header and body as version 2.
    let d = doc_with_block();
    let mut project = d.to_project();
    for l in &mut project.layers {
        l.id = 0; // v2 has no ids
    }
    let mut json = serde_json::to_value(&project).unwrap();
    for l in json["layers"].as_array_mut().unwrap() {
        let obj = l.as_object_mut().unwrap();
        for key in ["id", "opacity", "kind"] {
            obj.remove(key);
        }
    }
    json.as_object_mut().unwrap().remove("materials");
    let body = serde_json::to_vec(&json).unwrap();
    let mut file = b"RZXL".to_vec();
    file.extend_from_slice(&2u32.to_le_bytes());
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&zstd::encode_all(body.as_slice(), 3).unwrap());

    let project = rizxel_io::read_rzxl(file.as_slice()).unwrap();
    let loaded = Document::from_project(project, None).unwrap();
    assert_eq!(loaded.data.layers[0].id, 1);
    assert_eq!(loaded.data.layers[0].props, LayerProps::default());
    assert_eq!(loaded.visible_voxel_count(), 8);
}
