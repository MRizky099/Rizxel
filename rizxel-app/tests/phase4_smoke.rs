//! Phase 4 end-to-end check: voxel edits, undo/redo, save/load, meshing and
//! OBJ export through the headless application state.

use glam::IVec3;
use rizxel_app::{AppState, Document};
use rizxel_math::BoundSize;

/// A 4×3×2 block of one color plus a single voxel of another color on top.
fn pattern() -> Vec<(IVec3, u8)> {
    let mut cells: Vec<(IVec3, u8)> = (0..4)
        .flat_map(|x| (0..3).flat_map(move |y| (0..2).map(move |z| (IVec3::new(x, y, z), 9))))
        .collect();
    cells.push((IVec3::new(0, 3, 0), 20));
    cells
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rizxel_{name}_{}", std::process::id()))
}

#[test]
fn edit_undo_save_load_mesh_export() {
    let mut state = AppState::default();
    let doc = state.doc_mut().unwrap();
    doc.set_bounds(BoundSize::new(8, 8, 8), false);
    assert!(doc.set_voxels(0, pattern(), false));
    assert_eq!(doc.visible_voxel_count(), 25);

    // Block: one quad per side, except the top: the L-shape left around the
    // other voxel needs 2 quads. Plus 5 faces of the single voxel.
    let tris = doc.visible_mesh().triangle_count();
    assert_eq!(tris, (5 + 2 + 5) * 2);

    assert!(doc.undo());
    assert_eq!(doc.visible_voxel_count(), 0);
    assert!(doc.visible_mesh().is_empty());
    assert!(doc.redo());
    assert_eq!(doc.visible_voxel_count(), 25);
    assert_eq!(
        doc.occupied_bounds(),
        Some((IVec3::ZERO, IVec3::new(4, 4, 2)))
    );

    // Save, reopen in a fresh state, and compare data + mesh.
    let path = temp_path("smoke.rzxl");
    state.save_active_as(&path).unwrap();
    let mut reopened = AppState::default();
    reopened.open(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let a = state.doc().unwrap();
    let b = reopened.doc().unwrap();
    assert_eq!(a.data, b.data);
    assert_eq!(b.visible_voxel_count(), 25);
    assert_eq!(b.visible_mesh(), a.visible_mesh());

    // OBJ export of the visible layers.
    assert!(reopened.can_export_obj());
    let obj = temp_path("smoke.obj");
    let written = reopened.export_active_obj(&obj).unwrap();
    let text = std::fs::read_to_string(&obj).unwrap();
    std::fs::remove_file(&obj).ok();
    assert_eq!(written, tris);
    assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), tris);
}

#[test]
fn empty_document_cannot_export() {
    let state = AppState::default();
    assert!(!state.can_export_obj());
    assert!(state.export_active_obj(&temp_path("empty.obj")).is_err());
}

#[test]
fn hidden_layer_is_not_exported() {
    let mut doc = Document::new("t");
    doc.set_voxels(0, pattern(), false);
    doc.add_layer();
    doc.set_voxels(1, [(IVec3::new(6, 6, 6), 3)], false);
    let both = doc.visible_mesh().triangle_count();
    doc.toggle_layer_visible(1);
    assert_eq!(doc.visible_mesh().triangle_count(), both - 12);
}
