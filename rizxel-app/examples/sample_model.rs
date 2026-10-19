//! Writes a small demo project for checking the viewport by hand:
//!
//! ```text
//! cargo run -p rizxel-app --example sample_model -- target/sample.rzxl
//! cargo run -p rizxel-ui -- target/sample.rzxl
//! ```
//!
//! Layer 1 holds a stepped pyramid, layer 2 a ring of colored pillars, so
//! hiding a layer visibly removes part of the model. A group holds a sphere
//! shape layer and a cropped, half-transparent linked clone of it (`.rzxl` v3).

use std::path::PathBuf;

use glam::IVec3;
use rizxel_app::{Document, LayerProps};
use rizxel_math::{BoundSize, ShapeKind, VoxelBox};

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/sample.rzxl"));

    let mut doc = Document::new("sample");
    doc.set_bounds(BoundSize::new(24, 16, 24), false);

    // Stepped pyramid, one palette row per step.
    let mut pyramid = Vec::new();
    for step in 0..6 {
        let color = 8 + (step as u8 % 6) * 8 + 3;
        for x in step..24 - step {
            for z in step..24 - step {
                pyramid.push((IVec3::new(x, step, z), color));
            }
        }
    }
    doc.set_voxels(0, pyramid, false);

    // Pillars around the pyramid on a second layer.
    doc.add_layer();
    let mut pillars = Vec::new();
    for (i, (x, z)) in [(1, 1), (22, 1), (1, 22), (22, 22), (11, 0), (0, 11)]
        .into_iter()
        .enumerate()
    {
        for y in 0..10 + i as i32 {
            pillars.push((IVec3::new(x, y, z), 10 + i as u8 * 7));
        }
    }
    doc.set_voxels(1, pillars, false);

    // Version 3 layer features: a group holding a sphere shape layer and a
    // linked clone of it, cropped to its lower half and half transparent.
    let group = doc.add_group();
    let sphere = doc.add_shape_layer(
        ShapeKind::Sphere,
        VoxelBox::from_corners(IVec3::new(9, 6, 9), IVec3::new(14, 11, 14)),
        36,
    );
    let sphere_index = doc.active_layer;
    debug_assert_eq!(doc.data.layers[sphere_index].id, sphere);
    if let Some(clone) = doc.add_linked_clone(sphere_index, IVec3::new(-6, -3, -6)) {
        let index = doc.active_layer;
        debug_assert_eq!(doc.data.layers[index].id, clone);
        let props = LayerProps {
            opacity: 0.5,
            parent: Some(group),
            crop: Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::new(23, 5, 23))),
            ..doc.data.layers[index].props.clone()
        };
        doc.set_layer_props(index, props, false);
    }

    if let Err(e) = doc.save_to(&path) {
        eprintln!("failed to write {}: {e}", path.display());
        std::process::exit(1);
    }
    println!(
        "wrote {} ({} voxels, {} triangles)",
        path.display(),
        doc.visible_voxel_count(),
        doc.visible_mesh().triangle_count()
    );
}
