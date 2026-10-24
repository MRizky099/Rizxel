//! Phase 10b: reference images through the document (undoable edits, Show
//! All / Hide All, the work plane) and a `.rzxl` v4 save and reload.

use glam::{Vec2, Vec3};
use rizxel_app::{AppState, Document};
use rizxel_io::import_image;
use rizxel_math::{BoundSize, DepthMode, ImageData, RefPlane};

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rizxel_{name}_{}", std::process::id()))
}

/// A real `w`×`h` PNG, as the References panel imports it.
fn png(w: u32, h: u32) -> ImageData {
    let pixels = vec![128u8; (w * h * 4) as usize];
    let bytes = rizxel_io::png_io::encode_rgba(w, h, &pixels).unwrap();
    import_image(bytes).unwrap().image
}

fn doc() -> Document {
    let mut d = Document::new("t");
    d.set_bounds(BoundSize::new(20, 10, 8), false);
    d
}

#[test]
fn adding_and_placing_references_is_undoable() {
    let mut d = doc();
    let start = d.data.clone();
    d.add_reference("front", png(40, 20), RefPlane::Xy).unwrap();
    assert_eq!(d.data.references.len(), 1);
    assert_eq!(d.data.references[0].size, Vec2::new(20.0, 10.0));
    let added = d.data.clone();

    // A drag of the offset merges into one step.
    for z in 1..=5 {
        assert!(d.update_reference(0, z > 1, |r| r.offset.z = z as f32));
    }
    assert!(d.update_reference(0, false, |r| r.depth = DepthMode::OnTop));
    assert_eq!(d.history_entries().0.len(), 4, "bounds, add, drag, depth");
    assert!(d.undo());
    assert!(d.undo());
    assert_eq!(d.data, added, "the whole drag is one step");
    assert!(d.undo());
    assert_eq!(d.data, start);
    assert!(d.redo());
    // Invalid values are refused.
    assert!(!d.update_reference(0, false, |r| r.size.x = -1.0));
    assert!(!d.update_reference(0, false, |r| r.id = 99));
    assert!(!d.update_reference(5, false, |r| r.visible = false));
}

#[test]
fn show_all_hide_all_and_order() {
    let mut d = doc();
    d.add_reference("a", png(4, 4), RefPlane::Xy).unwrap();
    d.add_reference("b", png(4, 4), RefPlane::Yz).unwrap();
    d.add_reference("c", png(4, 4), RefPlane::Xz).unwrap();
    assert!(d.update_reference(1, false, |r| r.visible = false));
    let before = d.data.clone();
    // Some visible: Alt+R hides all, then shows all.
    assert!(d.toggle_all_references());
    assert!(d.data.references.iter().all(|r| !r.visible));
    assert!(d.toggle_all_references());
    assert!(d.data.references.iter().all(|r| r.visible));
    assert!(d.undo() && d.undo());
    assert_eq!(d.data, before);
    let (entries, position) = d.history_entries();
    assert_eq!(entries[position - 1], "Hide Reference");
    assert_eq!(entries[position], "Hide All References");

    assert!(d.move_reference(0, 2));
    let names: Vec<_> = d.data.references.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["b", "c", "a"]);
    assert_eq!(d.active_reference, 2);
    assert!(d.duplicate_reference(2));
    assert_eq!(d.data.references[3].name, "a copy");
    assert_ne!(d.data.references[3].id, d.data.references[2].id);
    assert_eq!(
        d.data.references[3].image.key(),
        d.data.references[2].image.key(),
        "the copy shares the image bytes"
    );
    assert!(d.remove_reference(3));
    assert_eq!(d.data.references.len(), 3);
}

#[test]
fn work_plane_aligns_to_a_reference() {
    let mut d = doc();
    d.add_reference("side", png(8, 8), RefPlane::Yz).unwrap();
    let bounds = d.data.bounds;
    assert!(d.update_reference(0, false, |r| r.snap_to_face(bounds, 1)));
    assert_eq!(d.data.references[0].offset.x, 10.0);
    assert!(d.align_work_plane_to_reference(0));
    assert_eq!((d.plane.axis, d.plane.coord), (0, 10));
    assert!(d.plane.visible);
    assert!(!d.align_work_plane_to_reference(4));
}

#[test]
fn replacing_an_image_keeps_the_placement() {
    let mut d = doc();
    d.add_reference("a", png(10, 10), RefPlane::Xy).unwrap();
    assert!(d.update_reference(0, false, |r| r.offset = Vec3::new(1.0, 2.0, 3.0)));
    let width = d.data.references[0].size.x;
    assert!(d.replace_reference_image(0, png(20, 5)));
    let r = &d.data.references[0];
    assert_eq!(r.offset, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(r.size, Vec2::new(width, width / 4.0));
    assert_eq!((r.image.width, r.image.height), (20, 5));
}

#[test]
fn references_survive_a_save_and_reload() {
    let mut state = AppState::default();
    let d = state.doc_mut().unwrap();
    d.add_reference("front", png(16, 8), RefPlane::Xy).unwrap();
    d.add_reference("side", png(8, 16), RefPlane::Yz).unwrap();
    d.update_reference(1, false, |r| {
        r.rotation = 3;
        r.flip_u = true;
        r.opacity = 0.25;
    });
    d.add_group();
    let path = temp_path("v4_references.rzxl");
    state.save_active_as(&path).unwrap();
    let mut reopened = AppState::default();
    reopened.open(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let (a, b) = (state.doc().unwrap(), reopened.doc().unwrap());
    assert_eq!(a.data, b.data);
    assert!(!b.is_dirty());
    assert_eq!(b.data.references[1].rotation, 3);
}

#[test]
fn reference_count_is_capped() {
    let mut d = doc();
    let image = png(2, 2);
    for _ in 0..rizxel_io::MAX_REFERENCES {
        assert!(d.add_reference("r", image.clone(), RefPlane::Xz).is_some());
    }
    assert!(d
        .add_reference("one too many", image, RefPlane::Xz)
        .is_none());
    assert!(!d.duplicate_reference(0));
}

#[test]
fn image_files_become_references() {
    let dir = std::env::temp_dir();
    let png_path = dir.join(format!("rizxel_ref_{}.png", std::process::id()));
    let pixels = vec![90u8; 6 * 3 * 4];
    std::fs::write(
        &png_path,
        rizxel_io::png_io::encode_rgba(6, 3, &pixels).unwrap(),
    )
    .unwrap();
    let mut state = AppState::default();
    let msg = state
        .add_reference_file(&png_path, Some(RefPlane::Xz))
        .unwrap();
    assert!(msg.contains("on XZ"), "{msg}");
    let d = state.doc().unwrap();
    assert_eq!(
        d.data.references[0].name,
        format!("rizxel_ref_{}.png", std::process::id())
    );
    assert_eq!(
        d.history_entries().0.last().unwrap(),
        &format!("Add Reference \"rizxel_ref_{}.png\"", std::process::id())
    );
    // The plane the camera faces most: the default camera looks at the front.
    state
        .camera
        .snap(rizxel_app::ViewTarget::Face(rizxel_app::ViewFace::Right));
    assert_eq!(state.facing_plane(), RefPlane::Yz);
    state.add_reference_file(&png_path, None).unwrap();
    assert_eq!(state.doc().unwrap().data.references[1].plane, RefPlane::Yz);
    // Replace keeps the placement; the same image again changes nothing.
    let offset = state.doc().unwrap().data.references[1].offset;
    assert!(state.replace_reference_file(1, &png_path).is_err());
    std::fs::write(
        &png_path,
        rizxel_io::png_io::encode_rgba(2, 2, &[7; 16]).unwrap(),
    )
    .unwrap();
    assert!(state.replace_reference_file(1, &png_path).is_ok());
    let r = &state.doc().unwrap().data.references[1];
    assert_eq!((r.image.width, r.offset), (2, offset));
    assert!(state.replace_reference_file(9, &png_path).is_err());
    // Not an image.
    let txt = dir.join(format!("rizxel_ref_{}.png.txt", std::process::id()));
    std::fs::write(&txt, b"plain text").unwrap();
    assert!(state.add_reference_file(&txt, None).is_err());
    assert!(rizxel_app::is_image_path(&png_path) && !rizxel_app::is_image_path(&txt));
    std::fs::remove_file(&png_path).ok();
    std::fs::remove_file(&txt).ok();
    // Alt+R hides, then shows.
    state.toggle_all_references();
    assert!(state
        .doc()
        .unwrap()
        .data
        .references
        .iter()
        .all(|r| !r.visible));
    state.toggle_all_references();
    assert!(state
        .doc()
        .unwrap()
        .data
        .references
        .iter()
        .all(|r| r.visible));
}
