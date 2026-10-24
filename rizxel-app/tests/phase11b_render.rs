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
