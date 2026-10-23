//! Phase 8: clipboard (copy, cut, floating paste, paste to new layer) and
//! one-shot transforms, each a single undo step.

use glam::IVec3;
use rizxel_app::{AppState, Clipboard, Document, Layer};
use rizxel_engine::MaskMode;
use rizxel_math::{BoundSize, Palette, Rgba8, Voxel, EMPTY_VOXEL};

/// A 3-voxel L at (1..3, 0, 0) + (1, 1, 0) with colors 2, 3 and 4.
fn l_layer() -> Layer {
    let mut l = Layer::new("A", BoundSize::default());
    l.volume.set(IVec3::new(1, 0, 0), 2);
    l.volume.set(IVec3::new(2, 0, 0), 3);
    l.volume.set(IVec3::new(1, 1, 0), 4);
    l
}

fn voxels(d: &Document, layer: usize) -> Vec<(IVec3, Voxel)> {
    let mut v: Vec<_> = d.data.layers[layer].volume.iter().collect();
    v.sort_by_key(|(p, _)| (p.z, p.y, p.x));
    v
}

fn state_with(doc: Document) -> AppState {
    let mut s = AppState::default();
    s.documents[0] = doc;
    s
}

#[test]
fn copy_and_paste_round_trip() {
    let mut s = state_with(Document::with_layers("t", vec![l_layer()]));
    assert!(s.copy(), "no selection copies the whole layer");
    let clip = s.clipboard.clone().unwrap();
    assert_eq!(clip.origin, IVec3::new(1, 0, 0));
    assert_eq!(clip.size, glam::UVec3::new(2, 2, 1));

    // Paste at the cursor: the box's bottom center lands on the cell.
    assert!(s.paste(Some(IVec3::new(10, 0, 10))));
    let doc = s.doc_mut().unwrap();
    assert_eq!(doc.floating_paste().unwrap().origin, IVec3::new(9, 0, 10));
    // It follows the cursor without leaving voxels behind.
    doc.move_paste(IVec3::new(20, 0, 20));
    assert_eq!(doc.data.layers[0].volume.voxel_count(), 6);
    assert_eq!(doc.data.layers[0].volume.get(IVec3::new(20, 0, 20)), 2);
    assert_eq!(doc.commit_paste(), 3);
    assert!(doc.floating_paste().is_none());
    assert_eq!(
        doc.selection().unwrap().len(),
        3,
        "the paste stays selected"
    );

    // One undo step removes the paste and its selection.
    assert!(doc.undo());
    assert_eq!(voxels(doc, 0).len(), 3);
    assert!(doc.selection().unwrap().is_empty());
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(doc.data.layers[0].volume.get(IVec3::new(21, 0, 20)), 3);
}

#[test]
fn copy_takes_only_the_selection() {
    let mut d = Document::with_layers("t", vec![l_layer()]);
    d.select([IVec3::new(2, 0, 0), IVec3::new(5, 5, 5)], MaskMode::Set);
    let clip = Clipboard::copy(&d).unwrap();
    assert_eq!(clip.cells.len(), 1, "empty selected cells are skipped");
    assert_eq!(clip.origin, IVec3::new(2, 0, 0));
}

#[test]
fn cut_is_one_undo_step() {
    let mut s = state_with(Document::with_layers("t", vec![l_layer()]));
    s.doc_mut()
        .unwrap()
        .select([IVec3::new(1, 0, 0), IVec3::new(2, 0, 0)], MaskMode::Set);
    let before = s.doc().unwrap().data.clone();
    assert!(s.cut());
    let doc = s.doc_mut().unwrap();
    assert_eq!(voxels(doc, 0), vec![(IVec3::new(1, 1, 0), 4)]);
    assert!(doc.undo());
    assert_eq!(doc.data, before);
    assert_eq!(s.clipboard.as_ref().unwrap().cells.len(), 2);

    // Without a selection the whole layer goes; locked layers refuse.
    let doc = s.doc_mut().unwrap();
    doc.clear_selection();
    doc.toggle_layer_locked(0);
    assert!(!s.cut());
    s.doc_mut().unwrap().toggle_layer_locked(0);
    assert!(s.cut());
    assert!(s.doc().unwrap().data.layers[0].volume.is_empty());
}

#[test]
fn paste_maps_colors_across_palettes() {
    let mut s = state_with(Document::with_layers("src", vec![l_layer()]));
    let src_palette = s.doc().unwrap().data.palette.clone();
    s.copy();

    // The target palette has color 3 at another index and lacks the rest.
    let c2 = src_palette.get(2).unwrap();
    let c3 = src_palette.get(3).unwrap();
    let mut target = Document::new("dst");
    target.data.palette = Palette::new(vec![
        Rgba8::BLACK,
        Rgba8::new(c2.r.saturating_add(3), c2.g, c2.b, 255),
        c3,
    ]);
    s.push_document(target);
    assert!(s.paste(None));
    s.commit_paste();
    let doc = s.doc().unwrap();
    let vol = &doc.data.layers[0].volume;
    assert_eq!(vol.get(IVec3::new(2, 0, 0)), 2, "exact match");
    assert_eq!(vol.get(IVec3::new(1, 0, 0)), 1, "nearest color");
}

#[test]
fn paste_is_clipped_by_the_bounds() {
    let mut s = state_with(Document::with_layers("t", vec![l_layer()]));
    s.copy();
    let edge = s.doc().unwrap().data.bounds.x() as i32 - 1;
    // Origin at the last column: the x+1 cells fall outside.
    s.paste(None);
    let doc = s.doc_mut().unwrap();
    doc.move_paste(IVec3::new(edge, 0, 5));
    assert_eq!(doc.commit_paste(), 2);
    assert_eq!(doc.data.layers[0].volume.get(IVec3::new(edge, 1, 5)), 4);
}

#[test]
fn cancelled_paste_leaves_no_history() {
    let mut s = state_with(Document::with_layers("t", vec![l_layer()]));
    s.copy();
    let before = s.doc().unwrap().data.clone();
    s.paste(Some(IVec3::new(8, 0, 8)));
    s.cancel_paste();
    let doc = s.doc_mut().unwrap();
    assert_eq!(doc.data, before);
    assert!(!doc.can_undo());
    // Undo while floating cancels the paste only.
    s.paste(Some(IVec3::new(8, 0, 8)));
    let doc = s.doc_mut().unwrap();
    assert!(doc.undo());
    assert_eq!(doc.data, before);
    assert!(doc.floating_paste().is_none());
}

#[test]
fn paste_to_new_layer_is_one_step() {
    let mut s = state_with(Document::with_layers("t", vec![l_layer()]));
    s.copy();
    assert!(s.paste_to_new_layer());
    let doc = s.doc_mut().unwrap();
    assert_eq!(doc.data.layers.len(), 2);
    assert_eq!(doc.active_layer, 1);
    assert_eq!(doc.data.layers[1].name, "Pasted");
    assert_eq!(voxels(doc, 1), voxels(doc, 0));
    assert_eq!(doc.selection().unwrap().len(), 3);
    assert!(doc.undo());
    assert_eq!(doc.data.layers.len(), 1);
    assert!(doc.data.layers[0].volume.get(IVec3::new(1, 1, 0)) != EMPTY_VOXEL);
}

/// An asymmetric 3-voxel L in the middle of the default bounds.
fn center_l() -> Document {
    let mut l = Layer::new("A", BoundSize::default());
    l.volume.set(IVec3::new(10, 10, 10), 2);
    l.volume.set(IVec3::new(11, 10, 10), 3);
    l.volume.set(IVec3::new(10, 11, 10), 4);
    Document::with_layers("t", vec![l])
}

#[test]
fn transforms_are_single_steps() {
    use rizxel_engine::tools::Transform;
    let mut d = center_l();
    let before = d.data.clone();
    // About Z (+90°): +X goes to +Y, +Y to -X, in the same 2×2 box.
    assert_eq!(d.transform(Transform::Rotate { axis: 2, turns: 1 }), Ok(4));
    assert_eq!(
        voxels(&d, 0),
        vec![
            (IVec3::new(10, 10, 10), 4),
            (IVec3::new(11, 10, 10), 2),
            (IVec3::new(11, 11, 10), 3),
        ]
    );
    assert!(d.undo());
    assert_eq!(d.data, before);
    assert!(!d.can_undo());

    // A selection is transformed alone and carried along.
    d.select(
        [IVec3::new(10, 10, 10), IVec3::new(11, 10, 10)],
        MaskMode::Set,
    );
    assert!(d.transform(Transform::Flip { axis: 0 }).is_ok());
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(10, 10, 10)), 3);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(11, 10, 10)), 2);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(10, 11, 10)), 4);
    assert_eq!(d.selection().unwrap().len(), 2);
    assert!(d.undo());
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(10, 10, 10)), 2);
}

#[test]
fn transforms_leaving_the_bounds_are_refused() {
    use rizxel_app::TransformError;
    use rizxel_engine::tools::Transform;
    let mut l = Layer::new("A", BoundSize::new(8, 8, 8));
    for y in 0..4 {
        l.volume.set(IVec3::new(0, y, 0), 2);
    }
    let mut d = Document::with_layers("t", vec![l]);
    d.set_bounds(BoundSize::new(8, 8, 8), false);
    let before = d.data.clone();
    let history = d.can_undo();
    assert_eq!(
        d.transform(Transform::Rotate { axis: 2, turns: 1 }),
        Err(TransformError::OutOfBounds)
    );
    assert_eq!(d.data, before);
    assert_eq!(d.can_undo(), history);
    d.toggle_layer_locked(0);
    assert!(matches!(
        d.transform(Transform::Flip { axis: 1 }),
        Err(TransformError::Blocked(_))
    ));
}

#[test]
fn mirror_copies_one_half_in_one_step() {
    let mut d = center_l();
    let before = d.data.clone();
    let size = d.data.bounds.x() as i32;
    assert!(d.mirror(0).is_ok());
    let vol = &d.data.layers[0].volume;
    assert_eq!(vol.get(IVec3::new(size - 1 - 11, 10, 10)), 3);
    assert_eq!(vol.voxel_count(), 6);
    assert!(d.undo());
    assert_eq!(d.data, before);
}

#[test]
fn rotate_facing_uses_the_camera_axis() {
    let mut s = state_with(center_l());
    s.camera
        .snap(rizxel_app::ViewTarget::Face(rizxel_app::ViewFace::Top));
    assert_eq!(s.facing_axis(), 1);
    s.rotate_facing(false);
    s.rotate_facing(true);
    let d = s.doc_mut().unwrap();
    assert_eq!(voxels(d, 0).len(), 3);
    assert!(d.undo() && d.undo());
    assert!(!d.can_undo(), "two steps");
}
