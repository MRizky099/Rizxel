//! Phase 5b: selections (undoable, per layer), selection-aware Move and
//! Extrude, Fill, Shape, and a randomized undo/redo round trip over all of
//! them (extended in Phase 8 with copy, cut, paste and the transforms, and in
//! Phase 10 with the selection operations, Mask Mode and the structure and
//! palette operations, in Phase 10b with the layer hierarchy and the
//! reference images, and in Phase 10c with box selection, selection to a
//! new layer, lines and curves and the Transform tool).

use glam::IVec3;
use rizxel_app::{
    Clipboard, CoreOp, Document, Edit, Layer, LayerDrop, PaletteSort, Selection, VoxelChange,
};
use rizxel_engine::tools::BooleanOp;
use rizxel_engine::tools::{
    apply_op, extrude_changes, extruded_cells, fill_cells, fuzzy_select, move_selection_changes,
    select_box, selection_box, selection_region, shape_box, shape_cells, Transform,
};
use rizxel_engine::{BrushShape, VoxelOp};
use rizxel_engine::{FillMode, FillOptions, FuzzySelectOptions, MaskMode, RayHit, ShapeOptions};
use rizxel_math::{BoundSize, Voxel, VoxelBox, EMPTY_VOXEL};

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

fn row(n: i32) -> Vec<IVec3> {
    (0..n).map(|x| IVec3::new(x, 0, 0)).collect()
}

#[test]
fn selection_edits_are_undoable_and_follow_the_mask() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    assert!(d.select(row(3), MaskMode::Set));
    assert_eq!(d.selection().unwrap().len(), 3);
    assert!(d.select([IVec3::new(3, 0, 0)], MaskMode::Add));
    assert!(d.select([IVec3::ZERO], MaskMode::Subtract));
    assert_eq!(d.selection().unwrap().len(), 3);
    assert!(!d.select([IVec3::splat(9)], MaskMode::Subtract), "no-op");
    assert!(d.clear_selection());
    assert!(!d.clear_selection());

    // Each change is one undo step restoring the exact set.
    assert!(d.undo());
    let expected: Selection = (1..4).map(|x| IVec3::new(x, 0, 0)).collect();
    assert_eq!(d.selection(), Some(&expected));
    d.undo();
    d.undo();
    assert_eq!(d.selection().unwrap().len(), 3);
    d.undo();
    assert!(d.selection().unwrap().is_empty());
    assert!(!d.can_undo());
    d.redo();
    assert_eq!(d.selection().unwrap().len(), 3);
}

#[test]
fn selections_are_per_layer_and_not_saved() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2), solid_layer("B", 3)]);
    d.active_layer = 0;
    d.select(row(2), MaskMode::Set);
    d.active_layer = 1;
    assert!(d.selection().unwrap().is_empty());
    // Reordering keeps each selection with its layer.
    d.move_layer(0, 1);
    assert_eq!(d.data.layers[1].name, "A");
    assert_eq!(d.data.layers[1].selection.len(), 2);
    // Duplicates start unselected.
    d.active_layer = 1;
    d.duplicate_active_layer();
    assert!(d.selection().unwrap().is_empty());

    let back = Document::from_project(d.to_project(), None).unwrap();
    assert!(back.data.layers.iter().all(|l| l.selection.is_empty()));
}

#[test]
fn moving_a_selection_moves_it_in_one_step() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.data.layers[0].volume.set(IVec3::new(0, 4, 0), 7);
    let top = [IVec3::new(0, 4, 0)];
    d.select(top, MaskMode::Set);
    let before = d.data.clone();

    let sel = d.selection().unwrap().clone();
    let offset = IVec3::new(2, 1, 0);
    d.begin_edit();
    let changes = move_selection_changes(&d.data.layers[0].volume, sel.cells(), offset, false)
        .expect("free space");
    d.edit_voxels(0, &changes);
    assert!(d.commit_edit_with_selection(0, sel.translated(offset, d.data.bounds)));
    assert_eq!(d.data.layers[0].volume.get(IVec3::new(2, 5, 0)), 7);
    assert_eq!(
        d.data.layers[0].volume.get(IVec3::new(0, 4, 0)),
        EMPTY_VOXEL
    );
    assert!(d.selection().unwrap().contains(IVec3::new(2, 5, 0)));
    assert_eq!(d.data.layers[0].volume.voxel_count(), 65, "the rest stays");

    assert!(d.undo());
    assert_eq!(d.data, before, "voxels and selection in one step");
    // Moving into the solid block is refused without overwrite.
    let blocked = move_selection_changes(&d.data.layers[0].volume, sel.cells(), -IVec3::Y, false);
    assert_eq!(blocked, None);
}

#[test]
fn extruding_a_selection_chains() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    // Select the top 2x2 corner of the block.
    let corner: Vec<IVec3> = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .iter()
        .map(|&(x, z)| IVec3::new(x, 3, z))
        .collect();
    d.select(corner, MaskMode::Set);
    for step in 1..=2 {
        let sel = d.selection().unwrap().clone();
        let volume = d.data.layers[0].volume.clone();
        let region = selection_region(&volume, sel.cells(), IVec3::Y);
        assert_eq!(region.len(), 4, "the top of the selection");
        d.begin_edit();
        d.edit_voxels(0, &extrude_changes(&volume, &volume, &region, IVec3::Y, 1));
        let mut grown = sel.clone();
        grown.extend(extruded_cells(&region, IVec3::Y, 1));
        d.commit_edit_with_selection(0, grown);
        assert_eq!(d.data.layers[0].volume.voxel_count(), 64 + 4 * step);
        assert_eq!(d.selection().unwrap().len(), 4 + 4 * step);
    }
    assert!(d.data.layers[0].volume.get(IVec3::new(1, 5, 1)) != EMPTY_VOXEL);
    d.undo();
    d.undo();
    assert_eq!(d.data.layers[0].volume.voxel_count(), 64);
    assert_eq!(d.selection().unwrap().len(), 4);
}

#[test]
fn delete_selected_respects_locks() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.select(row(4), MaskMode::Set);
    d.toggle_layer_locked(0);
    assert_eq!(d.delete_selected(), 0);
    d.toggle_layer_locked(0);
    assert_eq!(d.delete_selected(), 4);
    assert_eq!(d.data.layers[0].volume.voxel_count(), 60);
    assert!(d.selection().unwrap().is_empty());
    assert!(d.undo());
    assert_eq!(d.data.layers[0].volume.voxel_count(), 64);
    assert_eq!(d.selection().unwrap().len(), 4);
}

#[test]
fn fill_and_shape_are_single_steps() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    // Recolor the whole block (one color, connected) with color 5.
    let hit = RayHit {
        voxel: IVec3::new(1, 3, 1),
        normal: IVec3::Y,
        t: 0.0,
    };
    let all = VoxelBox::from_bounds(d.data.bounds);
    let opts = FillOptions::default();
    let cells = fill_cells(
        &d.data.layers[0].volume,
        &hit,
        &d.data.palette,
        &opts,
        &all,
        None,
    )
    .unwrap();
    let changes = apply_op(&d.data.layers[0].volume, cells, VoxelOp::Add, 5);
    assert_eq!(d.edit_voxels(0, &changes), 64);
    assert_eq!(d.data.layers[0].volume.get(IVec3::ZERO), 5);

    // A 3x3 sphere drag on top of the block.
    let area = shape_box(IVec3::new(0, 4, 0), IVec3::new(2, 4, 2), IVec3::Y, false);
    let opts = ShapeOptions {
        shape: BrushShape::Sphere,
        ..ShapeOptions::default()
    };
    let cells = shape_cells(&area, &opts, d.data.bounds);
    let changes = apply_op(&d.data.layers[0].volume, cells, VoxelOp::Add, 6);
    assert_eq!(d.edit_voxels(0, &changes), 19);

    assert!(d.undo());
    assert!(d.undo());
    assert_eq!(d.data.layers[0].volume.get(IVec3::ZERO), 2);
    assert_eq!(d.data.layers[0].volume.voxel_count(), 64);
}

/// Tiny deterministic generator (no external crates).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> i32 {
        (self.next() % n) as i32
    }

    fn cell(&mut self, n: u64) -> IVec3 {
        IVec3::new(self.below(n), self.below(n), self.below(n))
    }
}

/// One random tool operation on the active layer, applied like the UI does
/// (one transaction or one selection edit per operation). Returns which
/// operation it picked.
fn random_op(d: &mut Document, rng: &mut Lcg) -> usize {
    let layer = d.active_layer;
    let color = 1 + rng.below(8) as Voxel;
    let op = rng.below(OPS as u64);
    match op {
        // Brush-like stroke.
        0 => {
            let changes: Vec<VoxelChange> = (0..rng.below(6) + 1)
                .map(|_| VoxelChange {
                    pos: rng.cell(10),
                    old: 0,
                    new: color,
                })
                .collect();
            d.begin_edit();
            d.edit_voxels(layer, &changes);
            d.commit_edit();
        }
        // Box selection with a random mask.
        1 | 2 => {
            let area = selection_box(rng.cell(10), rng.cell(10));
            let cells = select_box(&[&d.data.layers[layer].volume], &area);
            let mode = MaskMode::ALL[rng.below(3) as usize];
            d.select(cells, mode);
        }
        // Fuzzy select from a random solid voxel.
        3 => {
            let volume = &d.data.layers[layer].volume;
            let opts = FuzzySelectOptions::default();
            let cells = volume
                .iter()
                .nth(rng.below(8) as usize)
                .map(|(seed, _)| fuzzy_select(volume, seed, &d.data.palette, &opts).unwrap());
            if let Some(cells) = cells {
                d.select(cells, MaskMode::Add);
            }
        }
        // Move the selection.
        4 => {
            let sel = d.data.layers[layer].selection.clone();
            let mut offset = glam::IVec3::ZERO;
            offset[rng.below(3) as usize] = rng.below(5) - 2;
            let overwrite = rng.below(2) == 0;
            if let Some(changes) =
                move_selection_changes(&d.data.layers[layer].volume, sel.cells(), offset, overwrite)
            {
                d.begin_edit();
                d.edit_voxels(layer, &changes);
                d.commit_edit_with_selection(layer, sel.translated(offset, d.data.bounds));
            }
        }
        // Extrude the selection up or carve it.
        5 => {
            let sel = d.data.layers[layer].selection.clone();
            let volume = d.data.layers[layer].volume.clone();
            let region = selection_region(&volume, sel.cells(), IVec3::Y);
            let depth = if rng.below(2) == 0 { 1 } else { -1 };
            d.begin_edit();
            d.edit_voxels(
                layer,
                &extrude_changes(&volume, &volume, &region, IVec3::Y, depth),
            );
            let cells = extruded_cells(&region, IVec3::Y, depth);
            let new = if depth > 0 {
                let mut grown = sel.clone();
                grown.extend(cells);
                grown
            } else {
                sel.combine(&cells.into_iter().collect(), MaskMode::Subtract)
            };
            d.commit_edit_with_selection(layer, new);
        }
        // Fill (recolor) inside the selection, if any.
        6 => {
            let volume = &d.data.layers[layer].volume;
            let allowed = d.data.layers[layer].selection.cells().clone();
            let allowed = (!allowed.is_empty()).then_some(&allowed);
            let opts = FillOptions {
                mode: FillMode::Recolor,
                ..FillOptions::default()
            };
            let all = VoxelBox::from_bounds(d.data.bounds);
            let changes = volume.iter().next().and_then(|(seed, _)| {
                let hit = RayHit {
                    voxel: seed,
                    normal: IVec3::Y,
                    t: 0.0,
                };
                fill_cells(volume, &hit, &d.data.palette, &opts, &all, allowed)
                    .ok()
                    .map(|cells| apply_op(volume, cells, VoxelOp::Add, color))
            });
            if let Some(changes) = changes {
                d.edit_voxels(layer, &changes);
            }
        }
        7 => {
            if rng.below(2) == 0 {
                d.delete_selected();
            } else {
                d.clear_selection();
            }
        }
        // Copy, then a floating paste that moves and is placed (or
        // cancelled); sometimes into another document's palette mapping.
        8 => {
            if let Some(clip) = Clipboard::copy(d) {
                let at = rng.cell(10) - IVec3::splat(2);
                if d.begin_paste(&clip, at, true).is_ok() {
                    d.move_paste(at + rng.cell(3));
                    if rng.below(4) == 0 {
                        d.cancel_paste();
                    } else {
                        d.commit_paste();
                    }
                }
            }
        }
        9 => {
            d.cut();
        }
        10 => {
            if let Some(clip) = Clipboard::copy(d) {
                d.paste_to_new_layer(&clip);
            }
        }
        11 => {
            let axis = rng.below(3) as usize;
            let turns = if rng.below(2) == 0 { 1 } else { -1 };
            let _ = d.transform(Transform::Rotate { axis, turns });
        }
        12 => {
            let _ = d.transform(Transform::Flip {
                axis: rng.below(3) as usize,
            });
        }
        13 => {
            let _ = d.mirror(rng.below(3) as usize);
        }
        // Phase 10: grow / shrink the selection, select by color.
        14 => {
            let steps = 1 + rng.below(2) as u32;
            if rng.below(2) == 0 {
                d.grow_selection(steps);
            } else {
                d.shrink_selection(steps);
            }
        }
        15 => {
            let mode = MaskMode::ALL[rng.below(3) as usize];
            d.select_by_color(2 + rng.below(2) as Voxel, 0, mode);
        }
        // Structure operations.
        16 => {
            if rng.below(2) == 0 {
                let _ = d.separate_parts(1 + rng.below(3) as usize);
            } else {
                d.plane.axis = rng.below(3) as usize;
                d.plane.coord = rng.below(6);
                let _ = d.split_at_plane();
            }
        }
        17 => {
            let op = match rng.below(3) {
                0 => CoreOp::FillHollows,
                1 => CoreOp::Hollow,
                _ => CoreOp::HollowAdvanced(1 + rng.below(2) as u32),
            };
            let _ = d.core_op(op);
        }
        18 => {
            let other = rng.below(d.data.layers.len() as u64) as usize;
            let _ = d.boolean(other, BooleanOp::ALL[rng.below(3) as usize]);
        }
        19 => {
            let _ = d.fit_bounds_to_content();
        }
        // Palette operations (they remap voxels on every layer).
        20 => {
            let key = [PaletteSort::Hue, PaletteSort::Luminance][rng.below(2) as usize];
            d.sort_palette(key);
        }
        21 => {
            d.remap_color(2 + rng.below(6) as usize, 2 + rng.below(6) as usize);
        }
        22 => {
            d.palette_gradient(1 + rng.below(20) as usize, 1 + rng.below(20) as usize);
        }
        23 => {
            // Snapping to a small palette with a repeated color leaves
            // duplicates for the next Remove Duplicates.
            if rng.below(2) == 0 {
                d.remove_duplicate_colors();
            } else {
                let c = rizxel_math::Rgba8::rgb(rng.below(256) as u8, 40, 9);
                let colors = [c, rizxel_math::Rgba8::rgb(9, rng.below(256) as u8, 90), c];
                d.snap_to_palette(&colors);
            }
        }
        // Phase 10b: layer hierarchy.
        24 => {
            let i = rng.below(d.data.layers.len() as u64) as usize;
            if d.data.layers[i].kind().is_group() && rng.below(2) == 0 {
                d.ungroup(i);
            } else {
                d.group_layer(i);
            }
        }
        25 => {
            let n = d.data.layers.len() as u64;
            let (from, row) = (rng.below(n) as usize, rng.below(n) as usize);
            match rng.below(4) {
                0 => d.drop_layer(from, LayerDrop::Above(row)),
                1 => d.drop_layer(from, LayerDrop::Below(row)),
                2 => d.drop_layer(from, LayerDrop::Into(row)),
                _ => {
                    d.active_layer = from;
                    d.move_active_layer(rng.below(2) == 0)
                }
            };
        }
        26 => {
            let groups: Vec<usize> = (0..d.data.layers.len())
                .filter(|&i| d.data.layers[i].kind().is_group())
                .collect();
            if !groups.is_empty() {
                let g = groups[rng.below(groups.len() as u64) as usize];
                match rng.below(3) {
                    0 => {
                        d.active_layer = g;
                        d.duplicate_active_layer();
                    }
                    1 => {
                        d.merge_group(g);
                    }
                    _ => {
                        d.delete_group_with_contents(g);
                    }
                }
            }
        }
        // Phase 10b: reference images.
        27 => {
            let n = d.data.references.len();
            let i = rng.below(n.max(1) as u64) as usize;
            match rng.below(6) {
                0 | 1 => {
                    let plane = rizxel_math::RefPlane::ALL[rng.below(3) as usize];
                    let image = rizxel_math::ImageData::new(vec![1, 2, 3], 4, 2);
                    d.add_reference("ref", image, plane);
                }
                2 => {
                    d.remove_reference(i);
                }
                3 => {
                    let z = rng.below(9) as f32;
                    let merge = rng.below(2) == 0;
                    d.update_reference(i, merge, |r| {
                        r.offset.z = z;
                        r.opacity = z / 8.0;
                        r.rotation = (r.rotation + 1) % 4;
                    });
                }
                4 => {
                    d.move_reference(i, rng.below(n.max(1) as u64) as usize);
                }
                _ => {
                    if rng.below(2) == 0 {
                        d.toggle_all_references();
                    } else {
                        d.duplicate_reference(i);
                    }
                }
            }
        }
        // Phase 10c: an open box selection, resized, then applied or
        // cancelled.
        28 => {
            let mode = MaskMode::ALL[rng.below(3) as usize];
            let all = rng.below(2) == 0;
            d.begin_box_select(selection_box(rng.cell(6), rng.cell(6)), all, mode);
            let area = selection_box(rng.cell(8), rng.cell(8));
            d.set_box_area(area, rng.below(2) == 0);
            if rng.below(5) == 0 {
                d.cancel_box_select();
            } else {
                d.apply_box_select();
            }
        }
        // Selection to a new layer, cut or copied.
        29 => {
            if d.selection().is_some_and(|s| s.is_empty()) {
                let area = selection_box(rng.cell(6), rng.cell(6));
                let cells = select_box(&[&d.data.layers[layer].volume], &area);
                d.select(cells, MaskMode::Set);
            } else {
                let _ = d.selection_to_new_layer(rng.below(2) == 0);
            }
        }
        _ => unreachable!("op {op}"),
    }
    op as usize
}

/// Number of operation kinds [`random_op`] picks from.
const OPS: usize = 30;

#[test]
fn random_tool_sequences_undo_and_redo_exactly() {
    // Operations that changed the document, per kind.
    let mut effective = [0usize; OPS];
    for seed in 1..=24u64 {
        let mut rng = Lcg(seed);
        let mut d = Document::with_layers("t", vec![solid_layer("A", 2), solid_layer("B", 3)]);
        // Start from a non-trivial selection on one layer (not undoable).
        d.data.layers[1].selection = (0..3).map(|x| IVec3::new(x, 3, 0)).collect();
        let initial = d.data.clone();
        for _ in 0..60 {
            d.active_layer = rng.below(d.data.layers.len() as u64) as usize;
            // Mask Mode (Phase 10) on for about a quarter of the operations.
            d.mask_selection = rng.below(4) == 0;
            let before = d.data.clone();
            let op = random_op(&mut d, &mut rng);
            assert!(d.data.structure_is_valid(), "seed {seed}: tree rules");
            // Every operation is a single undo step.
            if d.data != before {
                effective[op] += 1;
                let after = d.data.clone();
                assert!(d.undo());
                assert_eq!(d.data, before, "seed {seed}: one step");
                assert!(d.redo());
                assert_eq!(d.data, after, "seed {seed}: redo");
            }
        }
        let last = d.data.clone();
        while d.undo() {}
        assert_eq!(d.data, initial, "seed {seed}: undo all");
        while d.redo() {}
        assert_eq!(d.data, last, "seed {seed}: redo all");
        // The History panel's jumps land on the same states.
        let (entries, position) = d.history_entries();
        assert_eq!(position, entries.len());
        d.jump_to(0);
        assert_eq!(d.data, initial, "seed {seed}: jump to start");
        d.jump_to(entries.len());
        assert_eq!(d.data, last, "seed {seed}: jump to end");
    }
    assert!(
        effective.iter().all(|&n| n > 0),
        "every operation kind changed something: {effective:?}"
    );
}

#[test]
fn set_selection_edit_is_a_plain_swap() {
    let mut d = Document::new("t");
    let s: Selection = row(5).into_iter().collect();
    d.apply(
        Edit::SetSelection {
            layer: 0,
            old: Selection::default(),
            new: s.clone(),
        },
        false,
    );
    assert_eq!(d.selection(), Some(&s));
    d.undo();
    assert!(d.selection().unwrap().is_empty());
}
