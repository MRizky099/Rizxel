//! Writes demo projects made with Rizxel's own document API.
//!
//! ```text
//! cargo run -p rizxel-app --example sample_model -- target/sample.rzxl
//! cargo run -p rizxel-app --example sample_model -- --all assets/samples
//! cargo run -p rizxel-ui -- target/sample.rzxl
//! ```
//!
//! With a path, it writes the README's demo model. With `--all <dir>`, it
//! writes every sample model of `assets/samples/` (released under CC0):
//!
//! - `demo.rzxl`: a stepped pyramid, a ring of pillars and a group holding a
//!   sphere shape layer and a cropped, half-transparent linked clone of it.
//! - `lantern.rzxl`: a stone lantern with glass panes and a glowing flame,
//!   set up for Render mode (materials, night light, rounded voxels).
//! - `tree.rzxl`: a tree on a patch of grass; the crown is a group of
//!   sphere shape layers and linked clones.
//! - `cottage.rzxl`: a small house in layers (walls, roof, windows, door,
//!   chimney) with glass windows, rendered with the Brick voxel shape.
//! - `rizxel_r.rzxl`: the "R" of Rizxel's icon, set up with the *Voxel
//!   Icon* render preset (voxel lines, glowing floor grid, matte).

use std::path::{Path, PathBuf};

use glam::IVec3;
use rizxel_app::{Document, LayerProps};
use rizxel_math::render::{BackgroundMode, OutputKind, SkyMode, ToneMap};
use rizxel_math::{
    BoundSize, LayerId, Material, RenderPreset, Rgba8, ShapeKind, VoxelBox, VoxelShape,
};

/// Palette of the themed samples: slots 1.. in this order.
const COLORS: [(u8, u8, u8); 16] = [
    (0x6b, 0x6f, 0x76), // 1 stone
    (0x8d, 0x92, 0x99), // 2 light stone
    (0x4a, 0x4d, 0x52), // 3 dark stone
    (0xff, 0xb3, 0x47), // 4 flame
    (0xff, 0xe6, 0x8a), // 5 flame core
    (0xa8, 0xd8, 0xf0), // 6 glass
    (0x6e, 0x4b, 0x2e), // 7 bark
    (0x8a, 0x5f, 0x3a), // 8 light bark
    (0x3f, 0x8f, 0x3a), // 9 leaves
    (0x5c, 0xb8, 0x4a), // 10 light leaves
    (0x2d, 0x6b, 0x2b), // 11 dark leaves
    (0x7c, 0xc0, 0x5a), // 12 grass
    (0xd9, 0xc9, 0xa3), // 13 plaster
    (0xb0, 0x4a, 0x3a), // 14 roof tiles
    (0x5a, 0x3b, 0x26), // 15 door
    (0xe0, 0x5a, 0x4f), // 16 apple
];

const STONE: u8 = 1;
const LIGHT_STONE: u8 = 2;
const DARK_STONE: u8 = 3;
const FLAME: u8 = 4;
const FLAME_CORE: u8 = 5;
const GLASS: u8 = 6;
const BARK: u8 = 7;
const LIGHT_BARK: u8 = 8;
const LEAVES: u8 = 9;
const LIGHT_LEAVES: u8 = 10;
const DARK_LEAVES: u8 = 11;
const GRASS: u8 = 12;
const PLASTER: u8 = 13;
const ROOF: u8 = 14;
const DOOR: u8 = 15;
const APPLE: u8 = 16;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some("--all".as_ref()) {
        let dir = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("assets/samples"));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| fail(&dir, e));
        for (name, mut doc) in [
            ("demo", demo()),
            ("lantern", lantern()),
            ("tree", tree()),
            ("cottage", cottage()),
            ("rizxel_r", rizxel_r()),
        ] {
            save(&mut doc, &dir.join(format!("{name}.rzxl")));
        }
    } else {
        let path = first
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/sample.rzxl"));
        save(&mut demo(), &path);
    }
}

fn fail(path: &Path, e: impl std::fmt::Display) -> ! {
    eprintln!("failed to write {}: {e}", path.display());
    std::process::exit(1);
}

fn save(doc: &mut Document, path: &Path) {
    doc.save_to(path).unwrap_or_else(|e| fail(path, e));
    println!(
        "wrote {} ({} voxels, {} triangles)",
        path.display(),
        doc.visible_voxel_count(),
        doc.visible_mesh().triangle_count()
    );
}

/// New document with the themed palette.
fn themed(name: &str, bounds: BoundSize) -> Document {
    let mut doc = Document::new(name);
    doc.set_bounds(bounds, false);
    let colors: Vec<Rgba8> = COLORS
        .iter()
        .map(|&(r, g, b)| Rgba8::rgb(r, g, b))
        .collect();
    doc.load_palette(&colors);
    doc
}

/// Adds a voxel layer named `name` above the active layer and fills it. A
/// new layer joins the active layer's group, or the active group itself.
fn voxel_layer(
    doc: &mut Document,
    name: &str,
    cells: impl IntoIterator<Item = (IVec3, u8)>,
) -> usize {
    doc.add_layer();
    let index = doc.active_layer;
    doc.rename_layer(index, name.to_owned());
    doc.set_voxels(index, cells, false);
    index
}

/// Index of the layer with `id`.
fn index_of(doc: &Document, id: LayerId) -> usize {
    doc.data.layers.iter().position(|l| l.id == id).unwrap()
}

/// Gives the layer `index` a new material.
fn give_material(doc: &mut Document, index: usize, material: Material) {
    let slot = doc.add_material().expect("material slots left");
    doc.set_material(slot as usize, material, false);
    let props = LayerProps {
        material: Some(slot),
        ..doc.data.layers[index].props.clone()
    };
    doc.set_layer_props(index, props, false);
}

/// Every cell of the box from `a` to `b` (inclusive).
fn cuboid(a: IVec3, b: IVec3, color: u8) -> Vec<(IVec3, u8)> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut cells = Vec::new();
    for x in lo.x..=hi.x {
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                cells.push((IVec3::new(x, y, z), color));
            }
        }
    }
    cells
}

/// The letter of Rizxel's icon, front view, `#` = voxel. Row 0 is the top.
const LETTER_R: [&str; 15] = [
    "#########..",
    "##########.",
    "###########",
    "####...####",
    "####...####",
    "####...####",
    "###########",
    "##########.",
    "#########..",
    "####.####..",
    "####..####.",
    "####..####.",
    "####...####",
    "####...####",
    "####....###",
];

/// Rizxel's icon: a blue "R", 5 voxels deep, standing on a dark floor with
/// a glowing grid (the *Voxel Icon* render preset).
fn rizxel_r() -> Document {
    let depth = 5;
    let (w, h) = (LETTER_R[0].len() as i32, LETTER_R.len() as i32);
    let mut doc = Document::new("rizxel_r");
    doc.set_bounds(BoundSize::new(w as u32 + 2, h as u32 + 1, depth + 2), false);
    doc.set_palette_color(1, Rgba8::rgb(0x3a, 0x55, 0xf0), false);
    doc.rename_layer(0, "R".to_owned());
    let mut cells = Vec::new();
    for (row, line) in LETTER_R.iter().enumerate() {
        for (x, c) in line.chars().enumerate() {
            if c != '#' {
                continue;
            }
            for z in 1..=depth as i32 {
                cells.push((IVec3::new(x as i32 + 1, h - 1 - row as i32, z), 1));
            }
        }
    }
    doc.set_voxels(0, cells, false);
    let mut s = RenderPreset::VoxelIcon.apply(&doc.data.render);
    s.output.width = 1024;
    s.output.height = 1024;
    doc.set_render(s, false);
    doc
}

/// The README's demo model.
fn demo() -> Document {
    let mut doc = Document::new("demo");
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

    // A group holding a sphere shape layer and a linked clone of it,
    // cropped to its lower half and half transparent.
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
    doc
}

/// A stone lantern with glass panes and a flame, lit for Render mode.
fn lantern() -> Document {
    let mut doc = themed("lantern", BoundSize::new(15, 26, 15));
    doc.rename_layer(0, "Stone".to_owned());
    let mut stone = Vec::new();
    // Base, pillar and the lamp's floor.
    stone.extend(cuboid(
        IVec3::new(2, 0, 2),
        IVec3::new(12, 1, 12),
        DARK_STONE,
    ));
    stone.extend(cuboid(IVec3::new(4, 2, 4), IVec3::new(10, 3, 10), STONE));
    stone.extend(cuboid(IVec3::new(6, 4, 6), IVec3::new(8, 11, 8), STONE));
    stone.extend(cuboid(
        IVec3::new(3, 12, 3),
        IVec3::new(11, 13, 11),
        LIGHT_STONE,
    ));
    // Corner posts of the lamp.
    for (x, z) in [(3, 3), (11, 3), (3, 11), (11, 11)] {
        stone.extend(cuboid(IVec3::new(x, 14, z), IVec3::new(x, 19, z), STONE));
    }
    // Stepped roof and its knob.
    for (i, y) in (20..=23).enumerate() {
        let i = i as i32;
        stone.extend(cuboid(
            IVec3::new(1 + i * 2, y, 1 + i * 2),
            IVec3::new(13 - i * 2, y, 13 - i * 2),
            if i % 2 == 0 { DARK_STONE } else { STONE },
        ));
    }
    stone.extend(cuboid(
        IVec3::new(7, 24, 7),
        IVec3::new(7, 25, 7),
        LIGHT_STONE,
    ));
    doc.set_voxels(0, stone, false);

    // Glass panes between the posts.
    let mut glass = Vec::new();
    for y in 14..=19 {
        for t in 4..=10 {
            for p in [IVec3::new(t, y, 3), IVec3::new(t, y, 11)] {
                glass.push((p, GLASS));
            }
            for p in [IVec3::new(3, y, t), IVec3::new(11, y, t)] {
                glass.push((p, GLASS));
            }
        }
    }
    let panes = voxel_layer(&mut doc, "Glass", glass);
    give_material(
        &mut doc,
        panes,
        Material {
            roughness: 0.05,
            transmission: 0.95,
            ior: 1.5,
            tint: [0.85, 0.95, 1.0, 0.45],
            ..Material::new("Glass")
        },
    );

    // The flame: a glowing core inside a wider flame.
    let mut flame = cuboid(IVec3::new(6, 14, 6), IVec3::new(8, 15, 8), FLAME);
    flame.extend(cuboid(
        IVec3::new(7, 16, 7),
        IVec3::new(7, 18, 7),
        FLAME_CORE,
    ));
    flame.push((IVec3::new(7, 15, 7), FLAME_CORE));
    let flame = voxel_layer(&mut doc, "Flame", flame);
    give_material(
        &mut doc,
        flame,
        Material {
            emission: [1.0, 0.65, 0.25],
            emission_strength: 12.0,
            ..Material::new("Flame")
        },
    );

    // Dusk: a low, warm sun, a dark sky, rounded voxels.
    let mut render = doc.data.render.clone();
    render.light.sun_elevation = 8.0;
    render.light.sun_azimuth = 210.0;
    render.light.sun_intensity = 0.6;
    render.light.sun_color = [1.0, 0.7, 0.5];
    render.light.sky = SkyMode::Gradient;
    render.light.sky_color = [0.12, 0.12, 0.2];
    render.light.sky_zenith = [0.03, 0.04, 0.1];
    render.light.sky_intensity = 0.4;
    render.display.background = BackgroundMode::Sky;
    render.display.shape = VoxelShape::Rounded;
    render.display.ground_color = [0.25, 0.28, 0.22];
    render.camera.exposure = 0.5;
    render.camera.tone_map = ToneMap::Filmic;
    render.camera.bloom = true;
    render.output.kind = OutputKind::Photo;
    render.output.width = 1280;
    render.output.height = 1280;
    doc.set_render(render, false);
    doc
}

/// A tree on a patch of grass.
fn tree() -> Document {
    let mut doc = themed("tree", BoundSize::new(32, 36, 32));
    doc.rename_layer(0, "Grass".to_owned());
    let mut grass = Vec::new();
    for x in 0..32 {
        for z in 0..32 {
            let d = (IVec3::new(x, 0, z) - IVec3::new(16, 0, 16)).length_squared();
            if d <= 15 * 15 {
                let color = if (x * 7 + z * 13) % 5 == 0 {
                    LIGHT_LEAVES
                } else {
                    GRASS
                };
                grass.push((IVec3::new(x, 0, z), color));
            }
        }
    }
    doc.set_voxels(0, grass, false);

    // Trunk with roots and two branches.
    let mut trunk = cuboid(IVec3::new(15, 1, 15), IVec3::new(17, 14, 17), BARK);
    for (x, z) in [(14, 16), (18, 16), (16, 14), (16, 18)] {
        trunk.extend(cuboid(IVec3::new(x, 1, z), IVec3::new(x, 2, z), LIGHT_BARK));
    }
    for i in 0..6 {
        trunk.push((IVec3::new(18 + i, 12 + i, 16), BARK));
        trunk.push((IVec3::new(14 - i, 13 + i, 16), BARK));
    }
    voxel_layer(&mut doc, "Trunk", trunk);

    // The crown: a group of sphere shapes, one of them cloned. The first
    // layer goes into the new (active) group, the next ones next to it.
    let crown = doc.add_group();
    let crown_index = index_of(&doc, crown);
    doc.rename_layer(crown_index, "Crown".to_owned());
    let mut spheres = Vec::new();
    for (name, a, b, color) in [
        (
            "Crown Middle",
            IVec3::new(9, 15, 9),
            IVec3::new(23, 29, 23),
            LEAVES,
        ),
        (
            "Crown Top",
            IVec3::new(12, 24, 12),
            IVec3::new(20, 33, 20),
            LIGHT_LEAVES,
        ),
        (
            "Crown Left",
            IVec3::new(4, 16, 11),
            IVec3::new(13, 25, 20),
            DARK_LEAVES,
        ),
    ] {
        doc.add_shape_layer(ShapeKind::Sphere, VoxelBox::from_corners(a, b), color);
        let index = doc.active_layer;
        doc.rename_layer(index, name.to_owned());
        spheres.push(doc.data.layers[index].id);
    }
    // The right side mirrors the left with a linked clone.
    let left = index_of(&doc, spheres[2]);
    if doc.add_linked_clone(left, IVec3::new(15, 2, -1)).is_some() {
        let index = doc.active_layer;
        doc.rename_layer(index, "Crown Right (linked)".to_owned());
    }

    // A few apples, the crown group's top layer, so they show on the leaves.
    let apples = [
        IVec3::new(10, 20, 16),
        IVec3::new(22, 21, 13),
        IVec3::new(16, 19, 23),
        IVec3::new(19, 27, 9),
        IVec3::new(13, 24, 22),
    ];
    let apples = voxel_layer(&mut doc, "Apples", apples.map(|p| (p, APPLE)));
    give_material(
        &mut doc,
        apples,
        Material {
            roughness: 0.25,
            clearcoat: 0.6,
            ..Material::new("Apple")
        },
    );
    doc
}

/// A small cottage, layer by layer.
fn cottage() -> Document {
    let mut doc = themed("cottage", BoundSize::new(26, 24, 22));
    doc.rename_layer(0, "Ground".to_owned());
    doc.set_voxels(
        0,
        cuboid(IVec3::new(0, 0, 0), IVec3::new(25, 0, 21), GRASS),
        false,
    );
    // Every following layer goes into the group (see `voxel_layer`).
    let house = doc.add_group();
    let house_index = index_of(&doc, house);
    doc.rename_layer(house_index, "Cottage".to_owned());

    // Walls with a stone plinth, openings for the door and windows.
    let (lo, hi) = (IVec3::new(4, 1, 4), IVec3::new(21, 10, 17));
    let door = |p: IVec3| p.z == lo.z && (11..=13).contains(&p.x) && p.y <= 6;
    let window = |p: IVec3| {
        (4..=7).contains(&p.y)
            && ((p.z == lo.z || p.z == hi.z)
                && ((6..=8).contains(&p.x) || (16..=18).contains(&p.x))
                || (p.x == lo.x || p.x == hi.x) && (9..=12).contains(&p.z))
    };
    let mut walls = Vec::new();
    let mut panes = Vec::new();
    for (p, _) in cuboid(lo, hi, 0) {
        let shell = p.x == lo.x || p.x == hi.x || p.z == lo.z || p.z == hi.z;
        if !shell || door(p) {
            continue;
        }
        if window(p) {
            panes.push((p, GLASS));
            continue;
        }
        let color = if p.y <= 2 { STONE } else { PLASTER };
        walls.push((p, color));
    }
    voxel_layer(&mut doc, "Walls", walls);
    let windows = voxel_layer(&mut doc, "Windows", panes);
    give_material(
        &mut doc,
        windows,
        Material {
            roughness: 0.05,
            transmission: 0.9,
            tint: [0.9, 0.97, 1.0, 0.5],
            ..Material::new("Window Glass")
        },
    );
    voxel_layer(
        &mut doc,
        "Door",
        cuboid(IVec3::new(11, 1, lo.z), IVec3::new(13, 6, lo.z), DOOR),
    );

    // Pitched roof along X, one step per layer of height, with gables.
    let mut roof = Vec::new();
    let mut gables = Vec::new();
    for step in 0..=7 {
        let y = 11 + step;
        let (z0, z1) = (2 + step, 19 - step);
        for x in 2..=23 {
            roof.push((IVec3::new(x, y, z0), ROOF));
            roof.push((IVec3::new(x, y, z1), ROOF));
        }
        for z in z0 + 1..z1 {
            gables.push((IVec3::new(lo.x, y, z), PLASTER));
            gables.push((IVec3::new(hi.x, y, z), PLASTER));
        }
    }
    voxel_layer(&mut doc, "Roof", roof);
    voxel_layer(&mut doc, "Gables", gables);
    voxel_layer(
        &mut doc,
        "Chimney",
        cuboid(IVec3::new(17, 14, 13), IVec3::new(18, 21, 14), DARK_STONE),
    );

    let mut render = doc.data.render.clone();
    render.display.shape = VoxelShape::Brick;
    render.light.studio_lights = true;
    doc.set_render(render, false);
    doc
}
