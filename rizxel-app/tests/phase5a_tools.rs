//! Phase 5a: edit transactions (one drag = one undo step) and the Plane
//! tool's Slice mode across layers.

use glam::IVec3;
use rizxel_app::{Document, Layer, VoxelChange};
use rizxel_engine::tools::{brush_stroke, move_changes};
use rizxel_engine::{KeepSide, SliceRequest, ToolContext, ToolOptions, VoxelOp};
use rizxel_math::{BoundSize, Voxel};

fn set(pos: IVec3, new: Voxel) -> VoxelChange {
    VoxelChange { pos, old: 0, new }
}

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

#[test]
fn hundred_pushes_in_one_transaction_are_one_undo_step() {
    let mut d = Document::new("t");
    d.begin_edit();
    for i in 0..100 {
        let p = IVec3::new(i % 10, i / 10, 0);
        assert_eq!(d.edit_voxels(0, &[set(p, 3)]), 1);
    }
    // Repainting inside the drag keeps the original `old`.
    d.edit_voxels(0, &[set(IVec3::ZERO, 5)]);
    assert_eq!(d.pending_changes(), 100);
    assert!(d.commit_edit());
    assert!(!d.is_editing());
    assert_eq!(d.visible_voxel_count(), 100);
    assert!(d.undo());
    assert_eq!(d.visible_voxel_count(), 0);
    assert!(!d.can_undo(), "exactly one step");
    assert!(d.redo());
    assert_eq!(d.data.layers[0].volume.get(IVec3::ZERO), 5);
}

#[test]
fn cancel_restores_the_grid_exactly() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    let before = d.data.clone();
    d.begin_edit();
    d.edit_voxels(0, &[set(IVec3::splat(10), 7), set(IVec3::ZERO, 0)]);
    d.edit_voxels(0, &[set(IVec3::ONE, 9)]);
    assert_ne!(d.data, before);
    d.cancel_edit();
    assert_eq!(d.data, before);
    assert!(!d.can_undo(), "cancel records nothing");
}

#[test]
fn empty_transaction_records_nothing() {
    let mut d = Document::new("t");
    d.begin_edit();
    d.edit_voxels(0, &[set(IVec3::ZERO, 4)]);
    d.edit_voxels(0, &[set(IVec3::ZERO, 0)]);
    assert!(!d.commit_edit());
    assert!(!d.can_undo());
}

#[test]
fn revert_pending_recomputes_from_the_anchor() {
    // Plane-style tool: each frame reverts and rewrites the whole rectangle.
    let mut d = Document::new("t");
    d.begin_edit();
    for w in 1..=5 {
        d.revert_pending();
        let cells: Vec<VoxelChange> = (0..w).map(|x| set(IVec3::new(x, 0, 0), 1)).collect();
        d.edit_voxels(0, &cells);
    }
    d.revert_pending();
    d.edit_voxels(
        0,
        &[set(IVec3::new(0, 0, 0), 1), set(IVec3::new(1, 0, 0), 1)],
    );
    assert_eq!(d.pending_changes(), 2);
    assert!(d.commit_edit());
    assert_eq!(d.visible_voxel_count(), 2);
    d.undo();
    assert_eq!(d.visible_voxel_count(), 0);
}

#[test]
fn other_edits_and_undo_close_the_transaction_first() {
    let mut d = Document::new("t");
    d.begin_edit();
    d.edit_voxels(0, &[set(IVec3::ZERO, 4)]);
    d.add_layer();
    assert!(!d.is_editing());
    assert!(d.undo(), "undo the layer");
    assert!(d.undo(), "undo the drag");
    assert_eq!(d.visible_voxel_count(), 0);

    d.begin_edit();
    d.edit_voxels(0, &[set(IVec3::ZERO, 4)]);
    assert!(d.undo(), "undo mid-drag undoes the drag so far");
    assert!(!d.is_editing());
    assert_eq!(d.visible_voxel_count(), 0);
}

#[test]
fn locked_and_procedural_layers_refuse_tool_writes() {
    let mut d = Document::new("t");
    d.toggle_layer_locked(0);
    assert_eq!(d.edit_voxels(0, &[set(IVec3::ZERO, 4)]), 0);
    assert!(d.editable_layer(0).is_err());
    d.toggle_layer_locked(0);
    d.add_group();
    assert_eq!(d.edit_voxels(1, &[set(IVec3::ZERO, 4)]), 0);
    // Outside a transaction a write is its own undo step.
    assert_eq!(d.edit_voxels(0, &[set(IVec3::ZERO, 4)]), 1);
    assert!(!d.is_editing());
}

/// Tiny deterministic generator for the random op sequence.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

#[test]
fn undo_all_and_redo_all_over_random_drags() {
    let mut d = Document::new("t");
    let original = d.data.clone();
    let mut rng = Lcg(7);
    let options = ToolOptions::default();
    for _ in 0..40 {
        d.begin_edit();
        for _ in 0..1 + rng.next(6) {
            let p = IVec3::new(
                rng.next(12) as i32,
                rng.next(12) as i32,
                rng.next(12) as i32,
            );
            let op = [
                VoxelOp::Add,
                VoxelOp::Subtract,
                VoxelOp::Paint,
                VoxelOp::Join,
            ][rng.next(4) as usize];
            let changes = {
                let ctx = ToolContext {
                    volume: &d.data.layers[0].volume,
                    bounds: d.data.bounds,
                    color: 1 + rng.next(20) as Voxel,
                    op,
                    options: &options,
                    hit: None,
                };
                brush_stroke(&ctx, None, p)
            };
            d.edit_voxels(0, &changes);
        }
        if rng.next(5) == 0 {
            let offset = IVec3::new(rng.next(3) as i32 - 1, 0, 0);
            let changes = move_changes(&d.data.layers[0].volume, offset);
            d.edit_voxels(0, &changes);
        }
        d.commit_edit();
    }
    let last = d.data.clone();
    while d.undo() {}
    assert_eq!(d.data, original);
    while d.redo() {}
    assert_eq!(d.data, last);
}

#[test]
fn slice_spans_layers_and_skips_hidden_and_locked() {
    let layers = vec![
        solid_layer("A", 2),
        solid_layer("B", 3),
        solid_layer("Hidden", 4),
        solid_layer("Locked", 5),
    ];
    let mut d = Document::with_layers("t", layers);
    d.toggle_layer_visible(2);
    d.toggle_layer_locked(3);
    let untouched = (d.data.layers[2].clone(), d.data.layers[3].clone());
    let before = d.data.clone();

    let req = SliceRequest {
        axis: 0,
        cut: 2,
        keep: KeepSide::Positive,
        cap: false,
    };
    let out = d.slice(&req, 9);
    assert_eq!(out.layers, 2);
    assert_eq!(out.removed, 2 * 32);
    assert_eq!(out.locked_skipped, 1);
    assert_eq!(d.data.layers[0].volume.voxel_count(), 32);
    assert_eq!(d.data.layers[1].volume.voxel_count(), 32);
    assert_eq!(d.data.layers[2], untouched.0);
    assert_eq!(d.data.layers[3], untouched.1);

    // One undo restores both layers exactly; redo re-applies.
    let after = d.data.clone();
    assert!(d.undo());
    assert_eq!(d.data, before);
    assert!(d.redo());
    assert_eq!(d.data, after);

    // Capped slice adds a lid; slicing an already empty side is a no-op.
    let capped = d.slice(
        &SliceRequest {
            axis: 1,
            cut: 2,
            keep: KeepSide::Negative,
            cap: true,
        },
        9,
    );
    assert_eq!(capped.capped, 2 * 2 * 4);
    let none = d.slice(&req, 9);
    assert_eq!((none.layers, none.removed, none.capped), (0, 0, 0));
}

#[test]
fn slice_view_clips_the_mesh_and_cut_removes_the_hidden_side() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    let full = d.view_mesh().triangle_count();
    d.plane.axis = 0;
    d.plane.coord = 2;
    d.plane.slice = true;
    // Slice layer 1 and the far side behind it: a 2x4x4 box, still a
    // closed box of 6 greedy quads.
    assert_eq!(d.view_clip().unwrap().max.x, 2);
    assert_eq!(d.view_mesh().triangle_count(), full);
    assert_eq!(d.visible_voxel_count(), 64, "slicing is view-only");

    let out = d.cut_hidden_side(false, 9);
    assert_eq!(out.removed, 32);
    assert!(d.data.layers[0].volume.iter().all(|(p, _)| p.x < 2));
    assert!(d.undo());
    assert_eq!(d.visible_voxel_count(), 64);
}

#[test]
fn slice_edits_merge_back_when_slice_is_turned_off() {
    let mut d = Document::with_layers("t", vec![solid_layer("A", 2)]);
    d.plane.axis = 0;
    d.plane.slice = true;
    let bounds = d.data.bounds;
    // Slice layer 1: only x = 1 is editable.
    d.plane.set_slice_layer(1, bounds);
    let edit = d.edit_clip().unwrap();
    assert_eq!((edit.min.x, edit.max.x), (1, 2));
    // Paint over a voxel and add one in the slice layer; a write off the
    // slice is what tools filter away.
    let writes = [
        set(IVec3::new(1, 0, 0), 7),
        set(IVec3::new(1, 6, 0), 7),
        set(IVec3::new(0, 0, 0), 7),
    ];
    let inside: Vec<VoxelChange> = writes
        .into_iter()
        .filter(|c| edit.contains(c.pos))
        .collect();
    assert_eq!(inside.len(), 2);
    d.edit_voxels(0, &inside);

    d.plane.slice = false;
    assert_eq!(d.view_clip(), None);
    assert_eq!(d.edit_clip(), None);
    let v = &d.data.layers[0].volume;
    assert_eq!(v.get(IVec3::new(1, 0, 0)), 7, "latest edit wins");
    assert_eq!(v.get(IVec3::new(1, 6, 0)), 7);
    assert_eq!(v.get(IVec3::new(0, 0, 0)), 2, "off-slice voxel untouched");
    assert_eq!(v.voxel_count(), 65);
}

#[test]
fn undo_and_redo_move_the_slice_back_to_the_edit() {
    let mut d = Document::new("t");
    let bounds = d.data.bounds;
    d.plane.toggle_slice(None, bounds);
    d.plane.set_slice_layer(9, bounds);
    d.edit_voxels(0, &[set(IVec3::new(0, 9, 0), 3)]);
    // Slice moves are not undo steps.
    d.plane.set_slice_layer(0, bounds);
    d.plane.axis = 2;
    assert!(d.undo());
    assert_eq!((d.plane.axis, d.plane.slice_layer(bounds)), (1, 9));
    assert_eq!(d.visible_voxel_count(), 0);
    d.plane.set_slice_layer(2, bounds);
    assert!(d.redo());
    assert_eq!(d.plane.slice_layer(bounds), 9);
    assert_eq!(d.visible_voxel_count(), 1);
    // A step made without slicing leaves the slice alone.
    d.plane.toggle_slice(None, bounds);
    d.edit_voxels(0, &[set(IVec3::new(1, 1, 1), 3)]);
    d.plane.toggle_slice(None, bounds);
    d.plane.set_slice_layer(4, bounds);
    assert!(d.undo());
    assert_eq!(d.plane.slice_layer(bounds), 4);
}
