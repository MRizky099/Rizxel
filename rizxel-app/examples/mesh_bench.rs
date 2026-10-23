//! Re-mesh timing on a realistic model (Phase 5b performance pass):
//!
//! ```text
//! cargo run -p rizxel-app --example mesh_bench
//! cargo run -p rizxel-app --example mesh_bench --release
//! ```
//!
//! Builds a 128³ document holding about 50k voxels (a noisy terrain shell
//! plus a few solid blocks), then times what one drag frame costs: one
//! brush stamp through the edit transaction followed by `view_meshes`, as
//! the viewport does once per revision. `view_meshes` re-meshes only the
//! tiles the stamp touched; the full greedy mesh is timed for comparison,
//! without ambient occlusion (exports) and with it (viewport).

use std::time::Instant;

use glam::IVec3;
use rizxel_app::{Document, VoxelChange};
use rizxel_engine::{mesh_volume_with, MeshParams};
use rizxel_math::{BoundSize, Voxel};

/// Deterministic height field in `4..=20`.
fn height(x: i32, z: i32) -> i32 {
    let h = ((x as f32 * 0.11).sin() + (z as f32 * 0.07).cos()) * 6.0 + 12.0;
    h.round() as i32
}

fn main() {
    let mut doc = Document::new("bench");
    doc.set_bounds(BoundSize::new(128, 128, 128), false);
    let mut writes: Vec<(IVec3, Voxel)> = Vec::new();
    for z in 0..128 {
        for x in 0..128 {
            let h = height(x, z);
            // Three-voxel-thick surface shell, colored by height band.
            for y in (h - 2)..=h {
                writes.push((IVec3::new(x, y, z), (8 + (y / 4) * 8) as Voxel));
            }
        }
    }
    for (corner, size) in [(IVec3::new(20, 20, 20), 12), (IVec3::new(80, 22, 60), 10)] {
        for z in 0..size {
            for y in 0..size {
                for x in 0..size {
                    writes.push((corner + IVec3::new(x, y, z), 3));
                }
            }
        }
    }
    doc.set_voxels(0, writes, false);
    println!(
        "model: {} voxels in 128³",
        doc.data.layers[0].volume.voxel_count()
    );

    let start = Instant::now();
    let mesh = doc.visible_mesh();
    println!(
        "full mesh: {:.2} ms, {} triangles",
        start.elapsed().as_secs_f64() * 1e3,
        mesh.triangle_count()
    );
    let palette = doc.data.palette.clone();
    let ao = MeshParams::new(&palette).ao(true);
    let start = Instant::now();
    let mesh = mesh_volume_with(&doc.data.layers[0].volume, &ao);
    println!(
        "full mesh with AO: {:.2} ms, {} triangles",
        start.elapsed().as_secs_f64() * 1e3,
        mesh.triangle_count()
    );

    // The viewport has meshed the model before the drag starts.
    doc.view_meshes(false);

    // A drag: 30 frames, one 3³ stamp per frame along a line, each followed
    // by the re-mesh the viewport performs for the new revision.
    doc.begin_edit();
    let frames = 30;
    let mut total = 0.0;
    let mut worst: f64 = 0.0;
    for i in 0..frames {
        let center = IVec3::new(30 + i * 2, 40, 64);
        let stamp: Vec<VoxelChange> = (-1..=1)
            .flat_map(|dz| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| (dx, dy, dz))))
            .map(|(dx, dy, dz)| VoxelChange {
                pos: center + IVec3::new(dx, dy, dz),
                old: 0,
                new: 5,
            })
            .collect();
        let start = Instant::now();
        doc.edit_voxels(0, &stamp);
        let mesh = doc.view_meshes(false).opaque;
        let ms = start.elapsed().as_secs_f64() * 1e3;
        assert!(!mesh.is_empty());
        total += ms;
        worst = worst.max(ms);
    }
    doc.commit_edit();
    let full = mesh_volume_with(&doc.data.layers[0].volume, &ao);
    assert_eq!(doc.view_mesh(), full, "incremental == full");
    println!(
        "drag frame (stamp + re-mesh): avg {:.2} ms, worst {:.2} ms over {frames} frames",
        total / frames as f64,
        worst
    );
}
