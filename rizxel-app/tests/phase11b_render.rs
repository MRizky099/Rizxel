//! Phase 11b: render settings on the document, presets and per-layer
//! materials edited from the Render panel.

use rizxel_app::{Document, LayerProps};
use rizxel_math::{Material, MaterialPreset, RenderPreset, VoxelShape};

/// Document with three layers, the first two sharing material 0.
fn shared_material_doc() -> Document {
    let mut doc = Document::new("render");
    doc.add_layer();
    doc.add_layer();
    doc.set_materials(vec![Material::new("Plastic"), Material::new("Other")]);
    for (layer, material) in [(0, Some(0)), (1, Some(0)), (2, Some(1))] {
        let props = doc.data.layers[layer].props.clone();
        doc.set_layer_props(layer, LayerProps { material, ..props }, false);
    }
    doc
}

#[test]
fn render_settings_survive_save_and_load() {
    let mut doc = Document::new("render");
    let mut s = doc.data.render.clone();
    s.light.sun_azimuth = 120.0;
    s.display.shape = VoxelShape::Sphere;
    s.output.turntable_frames = 24;
    assert!(doc.set_render(s.clone(), false));
    let back = Document::from_project(doc.to_project(), None).unwrap();
    assert_eq!(back.data.render, s);
}

#[test]
fn render_edits_undo_and_merge_drags() {
    let mut doc = Document::new("render");
    let before = doc.data.render.clone();
    for azimuth in [50.0, 60.0, 70.0] {
        let mut s = doc.data.render.clone();
        s.light.sun_azimuth = azimuth;
        doc.set_render(s, true);
    }
    doc.commit_edit();
    assert_eq!(doc.data.render.light.sun_azimuth, 70.0);
    assert!(doc.is_dirty(), "render settings are saved in the file");
    assert!(doc.undo());
    assert_eq!(doc.data.render, before, "the drag is one undo step");
    assert!(!doc.can_undo());

    // Out-of-range values are clamped, unchanged values record nothing.
    let mut s = doc.data.render.clone();
    s.output.width = 0;
    doc.set_render(s, false);
    assert_eq!(doc.data.render.output.width, 1);
    let same = doc.data.render.clone();
    assert!(!doc.set_render(same, false));
}

#[test]
fn preset_is_one_undo_step() {
    let mut doc = Document::new("render");
    let before = doc.data.render.clone();
    assert!(doc.apply_render_preset(RenderPreset::StudioWhite));
    assert!(doc.data.render.display.shadow_catcher);
    let (entries, position) = doc.history_entries();
    assert_eq!(position, 1);
    assert_eq!(entries[0], "Render Preset \"Studio White\"");
    doc.undo();
    assert_eq!(doc.data.render, before);
}

#[test]
fn editing_a_shared_material_changes_every_layer_using_it() {
    let mut doc = shared_material_doc();
    assert_eq!(doc.material_users(0), 2);
    let mut m = doc.data.materials[0].clone();
    m.apply_preset(MaterialPreset::Metal);
    assert!(doc.set_material(0, m, false));
    assert_eq!(doc.layer_material(0).unwrap().metallic, 1.0);
    assert_eq!(doc.layer_material(1).unwrap().metallic, 1.0);
    assert_ne!(doc.layer_material(2).unwrap().metallic, 1.0);
}

#[test]
fn make_unique_changes_only_the_active_layer() {
    let mut doc = shared_material_doc();
    let unique = doc.make_material_unique(1).unwrap();
    assert_eq!(unique, 2);
    assert_eq!(doc.data.materials[2].name, "Plastic copy");
    assert_eq!(doc.material_users(0), 1);
    let mut m = doc.data.materials[2].clone();
    m.clearcoat = 1.0;
    doc.set_material(2, m, false);
    assert_eq!(doc.layer_material(1).unwrap().clearcoat, 1.0);
    assert_eq!(doc.layer_material(0).unwrap().clearcoat, 0.0);
    // Undo the edit, then the copy in one step.
    doc.undo();
    doc.undo();
    assert_eq!(doc.data.materials.len(), 2);
    assert_eq!(doc.data.layers[1].props.material, Some(0));
    // A layer on the default material gets a fresh one.
    let mut doc = Document::new("plain");
    assert_eq!(doc.make_material_unique(0), Some(0));
    assert_eq!(doc.data.layers[0].props.material, Some(0));
}

// ------------------------------------------------------------ shaped export

use rizxel_app::{AppState, FileFormat, ShapedExport, ViewMode, VoxelChange};
use rizxel_math::IVec3;

/// State with one document holding a 2×1×1 row of voxels.
fn two_bricks() -> AppState {
    let mut state = AppState::default();
    let doc = state.doc_mut().unwrap();
    let changes: Vec<_> = [IVec3::new(3, 0, 3), IVec3::new(4, 0, 3)]
        .into_iter()
        .map(|pos| VoxelChange {
            pos,
            old: 0,
            new: 12,
        })
        .collect();
    doc.begin_edit();
    doc.edit_voxels(0, &changes);
    doc.commit_edit();
    state
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rizxel_shaped_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn shaped_export_only_in_render_mode_with_a_shape() {
    let mut state = two_bricks();
    let path = temp("refused.obj");
    assert!(!state.can_export_shaped(), "Model mode");
    assert!(state
        .export_shaped(&path, &ShapedExport::default())
        .is_err());
    state.set_view_mode(ViewMode::Render);
    assert!(!state.can_export_shaped(), "the cube shape");
    let doc = state.doc_mut().unwrap();
    let mut s = doc.data.render.clone();
    s.display.shape = VoxelShape::Brick;
    doc.set_render(s, false);
    assert!(state.can_export_shaped());
}

#[test]
fn shaped_obj_matches_the_estimate_and_has_two_studs() {
    let mut state = two_bricks();
    state.set_view_mode(ViewMode::Render);
    let doc = state.doc_mut().unwrap();
    let mut s = doc.data.render.clone();
    s.display.shape = VoxelShape::Brick;
    doc.set_render(s, false);
    let export = ShapedExport::default();
    let doc = state.doc().unwrap();
    let estimate = doc.shaped_triangles(&doc.shape_options(&export));
    let mesh = doc.shaped_mesh(&doc.shape_options(&export));
    let path = temp("bricks.obj");
    state.export_shaped(&path, &export).unwrap();
    let obj = std::fs::read_to_string(&path).unwrap();
    let faces = obj.lines().filter(|l| l.starts_with("f ")).count();
    assert_eq!(faces, estimate);
    assert!(path.with_extension("mtl").exists());
    // Stud tops: upward normals above the brick tops (y = 1).
    let stud_tops: std::collections::BTreeSet<i32> = mesh
        .positions
        .iter()
        .zip(&mesh.normals)
        .filter(|(p, n)| n.y > 0.99 && p.y > 1.1)
        .map(|(p, _)| p.x.floor() as i32)
        .collect();
    assert_eq!(stud_tops.len(), 2);
    for ext in ["stl", "glb"] {
        let p = temp(&format!("bricks.{ext}"));
        state.export_shaped(&p, &export).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 100);
    }
    assert!(state.export_shaped(&temp("bricks.vox"), &export).is_err());
}

#[test]
fn model_exports_ignore_the_render_shape() {
    let mut state = two_bricks();
    let plain = temp("plain.obj");
    state.export_active(&plain, FileFormat::Obj).unwrap();
    let doc = state.doc_mut().unwrap();
    let mut s = doc.data.render.clone();
    s.display.shape = VoxelShape::Sphere;
    s.display.cell = 0.7;
    doc.set_render(s, false);
    let shaped = temp("plain_again.obj");
    state.export_active(&shaped, FileFormat::Obj).unwrap();
    let a = std::fs::read(&plain).unwrap();
    let b = std::fs::read(&shaped).unwrap();
    // Only the mtllib line names the file.
    let strip = |bytes: Vec<u8>| -> String {
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("mtllib"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(a), strip(b));
}

#[test]
fn render_mode_frames_through_the_render_lens() {
    let mut state = two_bricks();
    state.frame_selection();
    let model = state.camera.distance;
    state.set_view_mode(ViewMode::Render);
    let doc = state.doc_mut().unwrap();
    let mut s = doc.data.render.clone();
    s.camera.fov = 22.5;
    doc.set_render(s, false);
    state.frame_selection();
    // About tan(22.5°) / tan(11.25°) ≈ 2.08 times farther away.
    let ratio = state.camera.distance / model;
    assert!((ratio - 2.08).abs() < 0.05, "ratio {ratio}");
    assert!(state.hint().starts_with("Render mode"));
}
