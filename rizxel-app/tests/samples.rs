//! The sample models in `assets/samples/` (written by the `sample_model`
//! example) open as documents and show what they are meant to show.

use rizxel_app::Document;
use rizxel_math::LayerKind;

fn load(name: &str) -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../assets/samples")
        .join(name);
    Document::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn samples_open_with_their_layers() {
    for (name, min_voxels, layers) in [
        ("demo.rzxl", 1000, 5),
        ("lantern.rzxl", 500, 3),
        ("tree.rzxl", 1500, 8),
        ("cottage.rzxl", 1500, 8),
    ] {
        let doc = load(name);
        assert!(doc.visible_voxel_count() >= min_voxels, "{name}");
        assert_eq!(doc.data.layers.len(), layers, "{name}");
    }
    // The tree's crown is a group of shapes, a linked clone and the apples.
    let tree = load("tree.rzxl");
    let crown = tree.data.layers.iter().find(|l| l.name == "Crown").unwrap();
    let children: Vec<_> = tree
        .data
        .layers
        .iter()
        .filter(|l| l.props.parent == Some(crown.id))
        .collect();
    assert_eq!(children.len(), 5);
    assert!(children
        .iter()
        .any(|l| matches!(l.props.kind, LayerKind::Clone { .. })));
    // The lantern glows and has glass.
    let lantern = load("lantern.rzxl");
    assert!(lantern
        .data
        .materials
        .iter()
        .any(|m| m.emission_strength > 1.0));
    assert!(lantern.data.materials.iter().any(|m| m.transmission > 0.5));
}
