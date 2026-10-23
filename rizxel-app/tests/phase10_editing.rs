//! Phase 10: selection operations and Mask Mode, structure operations
//! (Separate Parts, Fit Bounds, Fill Hollows / Hollow, Booleans), palette
//! tools and the undo history list.

use glam::IVec3;
use rizxel_app::{
    AppState, CoreOp, Document, Layer, OpError, PaletteSort, Selection, TransformError, VoxelChange,
};
use rizxel_engine::tools::BooleanOp;
use rizxel_engine::tools::{apply_op, extrude_changes, extrude_region, fill_cells, Transform};
use rizxel_engine::{ExtrudeOptions, FillMode, FillOptions, MaskMode, RayHit, VoxelOp};
use rizxel_math::{BoundSize, Rgba8, Voxel, VoxelBox, EMPTY_VOXEL};

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

// ------------------------------------------------------------ Track B

/// Two 2³ cubes on one layer, three cells apart.
fn two_cubes() -> Document {
    let mut l = box_layer("A", IVec3::ZERO, IVec3::splat(2), 2);
    fill_box(&mut l, IVec3::new(5, 0, 0), IVec3::new(7, 2, 2), 3);
    l.volume.set(IVec3::new(10, 0, 0), 4);
    Document::with_layers("t", vec![l])
}

#[test]
fn separate_parts_gives_each_part_a_layer_in_one_step() {
    let mut d = two_cubes();
    let before = d.data.clone();
    // The single voxel stays: parts under 2 voxels are kept.
    assert_eq!(d.separate_parts(2), Ok(1));
    assert_eq!(d.data.layers.len(), 2);
    assert_eq!(d.data.layers[0].volume.voxel_count(), 9);
    assert_eq!(d.data.layers[1].volume.voxel_count(), 8);
    assert_eq!(d.data.layers[1].name, "A part 2");
    assert_eq!(d.active_layer, 0);
    d.undo();
    assert_eq!(d.data, before);
    assert_eq!(d.separate_parts(1), Ok(2));
    assert_eq!(d.data.layers.len(), 3);
    assert_eq!(
        d.separate_parts(1),
        Err(OpError::Nothing("The layer has only one part"))
    );
}

#[test]
fn split_at_the_work_plane_moves_the_plus_side() {
    let mut d = two_cubes();
    d.plane.axis = 0;
    d.plane.coord = 4;
    assert_eq!(d.split_at_plane(), Ok(1));
    assert_eq!(d.data.layers[0].volume.voxel_count(), 8);
    assert_eq!(d.data.layers[1].volume.voxel_count(), 9);
    d.plane.coord = 20;
    assert!(d.split_at_plane().is_err());
}

#[test]
fn fit_bounds_moves_content_to_the_origin_in_one_step() {
    let mut l = box_layer("A", IVec3::new(3, 4, 5), IVec3::new(6, 6, 7), 2);
    l.selection.insert(IVec3::new(3, 4, 5));
    let mut shape = Layer::new("S", BoundSize::default());
    shape.props.kind = rizxel_math::LayerKind::Shape {
        shape: rizxel_math::ShapeKind::Cube,
        area: VoxelBox::from_corners(IVec3::new(6, 4, 5), IVec3::new(7, 4, 5)),
        color: 3,
    };
    let mut d = Document::with_layers("t", vec![l, shape]);
    let before = d.data.clone();
    assert_eq!(d.fit_bounds_to_content(), Ok(BoundSize::new(5, 2, 2)));
    assert_eq!(d.data.bounds, BoundSize::new(5, 2, 2));
    let l = &d.data.layers[0];
    assert_eq!(l.volume.voxel_count(), 12);
    assert_eq!(l.volume.get(IVec3::ZERO), 2);
    assert!(l.selection.contains(IVec3::ZERO));
    assert_eq!(d.layer_geometry(1).unwrap().get(IVec3::new(3, 0, 0)), 3);
    assert!(d.fit_bounds_to_content().is_err(), "already fits");
    d.undo();
    assert_eq!(d.data, before);
}

#[test]
fn core_ops_on_the_active_layer() {
    let mut d = Document::with_layers("t", vec![box_layer("A", IVec3::ONE, IVec3::splat(6), 2)]);
    assert_eq!(d.core_op(CoreOp::Hollow), Ok(27));
    assert_eq!(d.data.layers[0].volume.voxel_count(), 98);
    d.selected_color = 5;
    assert_eq!(d.core_op(CoreOp::FillHollows), Ok(27));
    assert_eq!(d.data.layers[0].volume.get(IVec3::splat(3)), 5);
    assert!(d.core_op(CoreOp::FillHollows).is_err());
    d.undo();
    assert_eq!(d.data.layers[0].volume.voxel_count(), 98);
    d.undo();
    assert_eq!(d.data.layers[0].volume.voxel_count(), 125);
    assert_eq!(d.core_op(CoreOp::HollowAdvanced(2)), Ok(1));
}

#[test]
fn booleans_replace_only_the_active_layer() {
    let a = box_layer("A", IVec3::ZERO, IVec3::new(4, 1, 1), 2);
    let b = box_layer("B", IVec3::new(2, 0, 0), IVec3::new(6, 1, 1), 3);
    let mut d = Document::with_layers("t", vec![a, b]);
    let other = d.data.layers[1].clone();
    let row = |d: &Document| -> Vec<Voxel> {
        (0..6)
            .map(|x| d.data.layers[0].volume.get(IVec3::new(x, 0, 0)))
            .collect()
    };
    assert_eq!(d.boolean(1, BooleanOp::Union), Ok(2));
    assert_eq!(row(&d), [2, 2, 2, 2, 3, 3]);
    d.undo();
    assert_eq!(d.boolean(1, BooleanOp::Difference), Ok(2));
    assert_eq!(row(&d), [2, 2, 0, 0, 0, 0]);
    d.undo();
    assert_eq!(d.boolean(1, BooleanOp::Intersection), Ok(2));
    assert_eq!(row(&d), [0, 0, 2, 2, 0, 0]);
    assert_eq!(d.data.layers[1], other);
    assert!(d.boolean(0, BooleanOp::Union).is_err());
}

// ------------------------------------------------------------ Track C

/// Every voxel's RGBA, per layer: what the model looks like.
fn look(d: &Document) -> Vec<Vec<(IVec3, Rgba8)>> {
    d.data
        .layers
        .iter()
        .map(|l| {
            let mut cells: Vec<(IVec3, Rgba8)> = l
                .volume
                .iter()
                .map(|(p, v)| (p, d.data.palette.get(v as usize).unwrap()))
                .collect();
            cells.sort_by_key(|(p, _)| (p.x, p.y, p.z));
            cells
        })
        .collect()
}

/// Two layers painted with many palette colors.
fn colorful() -> Document {
    let mut a = Layer::new("A", BoundSize::default());
    let mut b = Layer::new("B", BoundSize::default());
    for i in 1..50 {
        a.volume.set(IVec3::new(i % 7, i / 7, 0), i as Voxel);
        b.volume.set(IVec3::new(i % 7, i / 7, 3), (50 - i) as Voxel);
    }
    b.locked = true;
    Document::with_layers("t", vec![a, b])
}

#[test]
fn sorting_the_palette_keeps_the_model_identical() {
    for key in [PaletteSort::Hue, PaletteSort::Luminance] {
        let mut d = colorful();
        d.selected_color = 35;
        let picked = d.selected_rgba();
        let (before, data) = (look(&d), d.data.clone());
        assert!(d.sort_palette(key));
        assert_ne!(d.data.palette, data.palette);
        assert_eq!(look(&d), before, "{key:?}");
        assert_eq!(d.selected_rgba(), picked, "selection follows its color");
        if key == PaletteSort::Luminance {
            let luma = |c: Rgba8| 2126 * c.r as u32 + 7152 * c.g as u32 + 722 * c.b as u32;
            let colors = &d.data.palette.colors()[1..];
            assert!(colors.windows(2).all(|w| luma(w[0]) <= luma(w[1])));
        }
        d.undo();
        assert_eq!(d.data, data, "one undo step");
    }
}

#[test]
fn gradient_endpoints_are_exact() {
    let mut d = colorful();
    let (a, b) = (10, 20);
    let (ca, cb) = (Rgba8::rgb(0, 100, 200), Rgba8::rgb(250, 0, 100));
    d.set_palette_color(a, ca, false);
    d.set_palette_color(b, cb, false);
    let before = d.data.palette.clone();
    assert!(d.palette_gradient(b, a));
    let p = &d.data.palette;
    assert_eq!((p.get(a), p.get(b)), (Some(ca), Some(cb)));
    assert_eq!(p.get(15), Some(Rgba8::rgb(125, 50, 150)));
    assert_eq!(p.get(9), before.get(9));
    assert_eq!(p.get(21), before.get(21));
    assert!(!d.palette_gradient(4, 5), "no slot between");
    d.undo();
    assert_eq!(d.data.palette, before);
}

#[test]
fn remap_touches_every_layer_in_one_step() {
    let mut d = colorful();
    let before = d.data.clone();
    // Color 7 is on layer A (x = 0, y = 1) and on layer B (43 → 50 - 43 = 7).
    let n = d.remap_color(7, 3);
    assert_eq!(n, 2);
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(0, 1, 0)), 3);
    assert_eq!(d.data.layers[1].volume.get(IVec3::new(1, 6, 3)), 3);
    assert_eq!(d.remap_color(7, 3), 0);
    d.undo();
    assert_eq!(d.data, before);
}

#[test]
fn removing_duplicates_closes_up_the_palette() {
    let mut d = colorful();
    let dup = d.data.palette.get(5).unwrap();
    d.set_palette_color(9, dup, false);
    d.set_palette_color(12, dup, false);
    let before = look(&d);
    let len = d.data.palette.len();
    assert_eq!(d.remove_duplicate_colors(), 2);
    assert_eq!(d.data.palette.len(), len - 2);
    assert_eq!(look(&d), before);
    assert_eq!(
        d.data.layers[0].volume.get(IVec3::new(2, 1, 0)),
        5,
        "9 became 5"
    );
    assert_eq!(d.remove_duplicate_colors(), 0);
}

#[test]
fn loading_and_snapping_palettes() {
    let mut d = colorful();
    let colors = [Rgba8::rgb(255, 0, 0), Rgba8::rgb(0, 0, 255)];
    assert!(d.load_palette(&colors));
    assert_eq!(d.data.palette.get(1), Some(colors[0]));
    assert_eq!(
        d.data.palette.get(3),
        colorful().data.palette.get(3),
        "rest kept"
    );
    d.undo();
    let mut d = colorful();
    d.set_palette_color(4, Rgba8::rgb(200, 10, 10), false);
    assert!(d.snap_to_palette(&colors));
    assert_eq!(d.data.palette.len(), 3);
    let v = d.data.layers[0].volume.get(IVec3::new(4, 0, 0));
    assert_eq!(d.data.palette.get(v as usize), Some(colors[0]));
    assert!(d
        .data
        .layers
        .iter()
        .all(|l| l.volume.iter().all(|(_, v)| v == 1 || v == 2)));
    d.undo();
    assert_eq!(d.data.palette.get(4), Some(Rgba8::rgb(200, 10, 10)));
}

#[test]
fn favorites_store_and_recall_palette_indices() {
    let mut s = AppState::default();
    s.doc_mut().unwrap().select_color(12);
    s.store_favorite(3);
    s.doc_mut().unwrap().select_color(20);
    s.recall_favorite(3);
    assert_eq!(s.doc().unwrap().selected_color, 12);
    s.recall_favorite(4);
    assert_eq!(s.doc().unwrap().selected_color, 12, "empty slot");
    s.new_document();
    s.recall_favorite(3);
    assert_eq!(s.doc().unwrap().selected_color, 12, "shared by documents");
}

#[test]
fn palette_files_load_through_the_app() {
    let dir = std::env::temp_dir().join(format!("rizxel_p10_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.hex");
    std::fs::write(&path, "ff0000\nnope\n00ff00\n").unwrap();
    let mut s = AppState::default();
    s.load_palette_file(&path, false).unwrap();
    assert_eq!(
        s.doc().unwrap().data.palette.get(2),
        Some(Rgba8::rgb(0, 255, 0))
    );
    assert!(s.status_message.as_deref().unwrap().contains("line 2"));
    let out = dir.join("out.gpl");
    s.save_palette_file(&out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("GIMP Palette"));
    std::fs::remove_dir_all(&dir).ok();
}
