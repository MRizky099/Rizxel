//! `.rzxl` format candidate fixtures (`tests/fixtures/rzxl_0_10/`, and
//! `tests/fixtures/rzxl_0_11/` for the fields added in version 6).
//!
//! The files were saved by Rizxel 0.10.0 (0.11.0) and are never regenerated: every
//! later build must load them and find the same contents. Together they use
//! every body field (see the compatibility promise in `docs/rzxl_format.md`).
//!
//! The expected projects are built below. Do not change these builders when
//! the format grows: a field added later must take its default when it is
//! missing, so the old files still compare equal to them.
//!
//! The files were written once with
//!
//! ```text
//! RIZXEL_WRITE_FIXTURES=1 cargo test -p rizxel-io --test rzxl_fixtures -- --ignored
//! ```
//!
//! which never overwrites an existing fixture.

use std::path::PathBuf;

use glam::{IVec3, Vec2, Vec3};
use rizxel_io::{
    load_rzxl, read_rzxl, runs_from_volume, volume_from_runs, write_rzxl, LayerData, ProjectFile,
    RZXL_VERSION,
};
use rizxel_math::render::{
    BackgroundMode, Denoise, OutputKind, PixelFilter, SkyMode, ToneMap, VoxelShape,
};
use rizxel_math::{
    BoundSize, DepthMode, ImageData, LayerKind, Material, Palette, RefPlane, ReferenceImage,
    RenderSettings, Rgba8, ShapeKind, SliceFilter, VoxelBox, VoxelVolume,
};

/// `generator` stored in every fixture.
const GENERATOR: &str = "Rizxel 0.10.0";
/// Format version of the 0.10.0 files.
const FIXTURE_VERSION: u32 = 5;

/// `generator` of the 0.11.0 fixtures.
const GENERATOR_0_11: &str = "Rizxel 0.11.0";
/// Format version of the 0.11.0 files.
const FIXTURE_VERSION_0_11: u32 = 6;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rzxl_0_10")
}

fn fixture_dir_0_11() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rzxl_0_11")
}

fn project(name: &str, bounds: BoundSize, layers: Vec<LayerData>) -> ProjectFile {
    ProjectFile {
        generator: GENERATOR.into(),
        name: name.into(),
        palette: Palette::default(),
        selected_color: Palette::DEFAULT_SELECTED,
        materials: Vec::new(),
        layers,
        active_layer: 0,
        bounds,
        references: Vec::new(),
        render: RenderSettings::default(),
    }
}

fn voxel_layer(id: u32, name: &str, bounds: BoundSize, cells: &[(IVec3, u8)]) -> LayerData {
    let mut volume = VoxelVolume::new(bounds);
    for &(p, c) in cells {
        volume.set(p, c);
    }
    LayerData {
        voxels: runs_from_volume(bounds, &volume),
        ..LayerData::new(id, name)
    }
}

/// Stepped pyramid of `steps` levels, one color per level.
fn pyramid(size: i32, steps: i32) -> Vec<(IVec3, u8)> {
    let mut cells = Vec::new();
    for step in 0..steps {
        for x in step..size - step {
            for z in step..size - step {
                cells.push((IVec3::new(x, step, z), 9 + step as u8 * 8));
            }
        }
    }
    cells
}

/// One empty voxel layer, default everything.
fn minimal() -> ProjectFile {
    project(
        "minimal",
        BoundSize::new(32, 32, 32),
        vec![LayerData::new(1, "Layer 1")],
    )
}

/// Voxel layers, hidden and locked layers, opacity, nested groups (one
/// collapsed), every shape kind, linked clones of a voxel and a shape
/// layer, and crop boxes. Each group's subtree sits directly below it.
fn layers() -> ProjectFile {
    let bounds = BoundSize::new(24, 16, 24);
    let mut hidden = voxel_layer(
        2,
        "Hidden Pillars",
        bounds,
        &(0..10)
            .map(|y| (IVec3::new(1, y, 1), 60))
            .collect::<Vec<_>>(),
    );
    hidden.visible = false;
    hidden.locked = true;
    hidden.opacity = 0.4;
    let shape = |id, name: &str, shape, a, b, color| LayerData {
        kind: LayerKind::Shape {
            shape,
            area: VoxelBox::from_corners(a, b),
            color,
        },
        parent: Some(4),
        ..LayerData::new(id, name)
    };
    let mut p = project(
        "layers",
        bounds,
        vec![
            voxel_layer(1, "Pyramid", bounds, &pyramid(24, 6)),
            hidden,
            shape(
                5,
                "Cube",
                ShapeKind::Cube,
                IVec3::new(2, 6, 2),
                IVec3::new(5, 9, 5),
                20,
            ),
            LayerData {
                crop: Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::new(23, 10, 23))),
                ..shape(
                    6,
                    "Sphere",
                    ShapeKind::Sphere,
                    IVec3::new(9, 6, 9),
                    IVec3::new(14, 11, 14),
                    36,
                )
            },
            shape(
                7,
                "Cylinder",
                ShapeKind::Cylinder,
                IVec3::new(17, 6, 17),
                IVec3::new(21, 15, 21),
                44,
            ),
            LayerData {
                kind: LayerKind::Group { collapsed: true },
                parent: Some(3),
                ..LayerData::new(4, "Shapes")
            },
            LayerData {
                kind: LayerKind::Clone {
                    source: 1,
                    offset: IVec3::new(0, 6, 0),
                },
                crop: Some(VoxelBox::from_corners(
                    IVec3::new(0, 6, 0),
                    IVec3::new(11, 15, 11),
                )),
                parent: Some(3),
                opacity: 0.75,
                ..LayerData::new(8, "Pyramid (linked, cropped)")
            },
            LayerData {
                kind: LayerKind::Clone {
                    source: 6,
                    offset: IVec3::new(-6, -3, -6),
                },
                parent: Some(3),
                opacity: 0.5,
                ..LayerData::new(9, "Sphere (linked)")
            },
            LayerData {
                kind: LayerKind::Group { collapsed: false },
                ..LayerData::new(3, "Props")
            },
        ],
    );
    p.active_layer = 6;
    p.selected_color = 44;
    p
}

/// A custom 16-color palette, a material per layer (every material field)
/// and a layer without material.
fn materials() -> ProjectFile {
    let bounds = BoundSize::new(16, 8, 16);
    let row = |z: i32, color: u8| -> Vec<(IVec3, u8)> {
        (0..16).map(|x| (IVec3::new(x, 0, z), color)).collect()
    };
    let mut p = project(
        "materials",
        bounds,
        vec![
            LayerData {
                material: Some(0),
                ..voxel_layer(1, "Matte", bounds, &row(0, 1))
            },
            LayerData {
                material: Some(1),
                ..voxel_layer(2, "Glass", bounds, &row(2, 2))
            },
            LayerData {
                material: Some(2),
                ..voxel_layer(3, "Neon", bounds, &row(4, 3))
            },
            LayerData {
                material: Some(3),
                ..voxel_layer(4, "Lacquered Metal", bounds, &row(6, 4))
            },
            voxel_layer(5, "Default", bounds, &row(8, 15)),
        ],
    );
    p.palette = Palette::new(
        (0..16u8)
            .map(|i| Rgba8::new(i * 16, 255 - i * 16, i * 8, if i == 0 { 0 } else { 255 }))
            .collect(),
    );
    p.selected_color = 3;
    p.active_layer = 2;
    p.materials = vec![
        Material {
            roughness: 1.0,
            ..Material::new("Matte")
        },
        Material {
            roughness: 0.05,
            tint: [0.8, 0.95, 1.0, 0.35],
            specular: 0.6,
            ior: 1.45,
            transmission: 0.9,
            ..Material::new("Glass")
        },
        Material {
            emission: [0.2, 0.8, 1.0],
            emission_strength: 6.5,
            ..Material::new("Neon")
        },
        Material {
            metallic: 1.0,
            roughness: 0.3,
            clearcoat: 0.75,
            specular: 0.25,
            ..Material::new("Lacquered Metal")
        },
    ];
    p
}

/// `w`×`h` gradient image, PNG or JPEG.
fn image(w: u32, h: u32, jpeg: bool) -> ImageData {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255]);
    }
    let mut bytes = Vec::new();
    if jpeg {
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
            .encode_image(&rgb)
            .unwrap();
    } else {
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    ImageData::new(bytes, w, h)
}

/// Reference images on all three planes, PNG and JPEG, with every
/// placement and display option.
fn references() -> ProjectFile {
    let bounds = BoundSize::new(32, 24, 16);
    let mut p = project(
        "references",
        bounds,
        vec![voxel_layer(1, "Body", bounds, &pyramid(16, 4))],
    );
    p.references = vec![
        ReferenceImage::fitted(1, "front.png", image(16, 8, false), RefPlane::Xy, bounds),
        ReferenceImage {
            rotation: 1,
            flip_u: true,
            opacity: 0.8,
            depth: DepthMode::OnTop,
            slice: SliceFilter::SliceOnly,
            locked: true,
            ..ReferenceImage::fitted(4, "top.jpg", image(12, 12, true), RefPlane::Xz, bounds)
        },
        ReferenceImage {
            id: 9,
            name: "side.png".into(),
            image: image(10, 20, false),
            visible: false,
            locked: false,
            plane: RefPlane::Yz,
            offset: Vec3::new(-2.0, 1.5, 3.25),
            size: Vec2::new(12.0, 24.0),
            rotation: 3,
            flip_u: false,
            flip_v: true,
            opacity: 0.25,
            depth: DepthMode::Behind,
            slice: SliceFilter::HiddenInSlice,
        },
    ];
    p.references[1].snap_to_face(bounds, 1);
    p
}

/// Every render setting away from its default.
fn render() -> ProjectFile {
    let bounds = BoundSize::new(16, 16, 16);
    let mut p = project(
        "render",
        bounds,
        vec![voxel_layer(1, "Model", bounds, &pyramid(16, 5))],
    );
    let r = &mut p.render;
    let l = &mut r.light;
    l.sun = false;
    l.sun_azimuth = -120.0;
    l.sun_elevation = 22.5;
    l.sun_size = 12.0;
    l.sun_intensity = 7.5;
    l.sun_color = [1.0, 0.8, 0.6];
    l.sky = SkyMode::Image;
    l.sky_color = [0.1, 0.2, 0.3];
    l.sky_zenith = [0.3, 0.4, 0.9];
    l.sky_intensity = 2.5;
    l.sky_rotation = 90.0;
    l.sky_image = "skies/studio.hdr".into();
    l.fog = true;
    l.fog_density = 0.05;
    l.fog_color = [0.5, 0.55, 0.6];
    l.studio_lights = true;
    let s = &mut r.sample;
    s.samples = 64;
    s.diffuse_bounces = 5;
    s.specular_bounces = 2;
    s.transmission_bounces = 12;
    s.russian_roulette = false;
    s.clamp = 25.0;
    s.filter = PixelFilter::Box;
    s.pixelated = true;
    s.denoise = Denoise::Off;
    let d = &mut r.display;
    d.ground = false;
    d.ground_color = [0.2, 0.3, 0.2];
    d.shadow_catcher = true;
    d.background = BackgroundMode::Transparent;
    d.background_color = [0.9, 0.1, 0.1];
    d.background_top = [0.1, 0.1, 0.9];
    d.bevel = 0.2;
    d.seams = true;
    d.edges = true;
    d.edge_color = [1.0, 1.0, 0.0];
    d.edge_width = 2.5;
    d.shape = VoxelShape::Rounded;
    d.cell = 0.75;
    let c = &mut r.camera;
    c.fov = 30.0;
    c.aperture = 1.5;
    c.focus_distance = 20.0;
    c.blades = 6;
    c.exposure = -1.25;
    c.tone_map = ToneMap::Filmic;
    c.gamma = 2.4;
    c.vignette = 0.3;
    c.bloom = true;
    c.bloom_threshold = 2.0;
    c.bloom_strength = 0.4;
    let o = &mut r.output;
    o.kind = OutputKind::Turntable;
    o.width = 800;
    o.height = 600;
    o.transparent = true;
    o.folder = "renders".into();
    o.name = "{name}_spin".into();
    o.time_limit = 30.0;
    o.png16 = true;
    o.turntable_frames = 24;
    o.turntable_angle = 180.0;
    o.turntable_path_traced = true;
    o.turntable_samples = 16;
    o.turntable_png = false;
    o.turntable_gif = false;
    o.turntable_sheet = true;
    o.gif_fps = 24;
    p
}

/// The largest bounds (256³) with voxels in its corners and long runs.
fn max_bounds() -> ProjectFile {
    let bounds = BoundSize::new(256, 256, 256);
    let mut cells: Vec<(IVec3, u8)> = [0, 255]
        .into_iter()
        .flat_map(|x| [0, 255].into_iter().map(move |y| (x, y)))
        .flat_map(|(x, y)| {
            [0, 255]
                .into_iter()
                .map(move |z| (IVec3::new(x, y, z), 200))
        })
        .collect();
    cells.extend((0..256).map(|y| (IVec3::new(128, y, 128), 255)));
    project(
        "max_bounds",
        bounds,
        vec![voxel_layer(1, "Corners", bounds, &cells)],
    )
}

/// Version 6: every display field added for the *Voxel Icon* look away
/// from its default.
fn voxel_icon() -> ProjectFile {
    let bounds = BoundSize::new(12, 12, 12);
    let mut p = project(
        "voxel_icon",
        bounds,
        vec![voxel_layer(1, "Model", bounds, &pyramid(12, 4))],
    );
    p.generator = GENERATOR_0_11.into();
    let d = &mut p.render.display;
    d.voxel_lines = true;
    d.voxel_line_color = [0.1, 0.2, 0.3];
    d.voxel_line_width = 0.06;
    d.voxel_line_opacity = 0.5;
    d.floor_grid = true;
    d.floor_grid_color = [0.9, 0.5, 0.2];
    d.floor_grid_width = 0.08;
    d.floor_grid_spacing = 4;
    d.floor_grid_fade = 2.5;
    d.floor_grid_glow = 1.25;
    d.matte = true;
    p
}

/// Every 0.11.0 fixture, by file name.
fn fixtures_0_11() -> Vec<(&'static str, ProjectFile)> {
    vec![("voxel_icon.rzxl", voxel_icon())]
}

/// Every fixture, by file name.
fn fixtures() -> Vec<(&'static str, ProjectFile)> {
    vec![
        ("minimal.rzxl", minimal()),
        ("layers.rzxl", layers()),
        ("materials.rzxl", materials()),
        ("references.rzxl", references()),
        ("render.rzxl", render()),
        ("max_bounds.rzxl", max_bounds()),
    ]
}

/// Every fixture with its folder, format version and generator.
fn all_fixtures() -> Vec<(PathBuf, u32, &'static str, ProjectFile)> {
    let mut all: Vec<_> = fixtures()
        .into_iter()
        .map(|(f, p)| (fixture_dir().join(f), FIXTURE_VERSION, GENERATOR, p))
        .collect();
    all.extend(fixtures_0_11().into_iter().map(|(f, p)| {
        (
            fixture_dir_0_11().join(f),
            FIXTURE_VERSION_0_11,
            GENERATOR_0_11,
            p,
        )
    }));
    all
}

#[test]
fn fixtures_load_with_their_saved_contents() {
    for (path, version, generator, expected) in all_fixtures() {
        let file = path.display();
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("{file}: {e} (fixtures are committed)"));
        assert_eq!(&bytes[0..4], b"RZXL", "{file}");
        assert_eq!(&bytes[4..8], &version.to_le_bytes(), "{file}");
        let loaded = load_rzxl(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(loaded.generator, generator, "{file}");
        assert_eq!(loaded, expected, "{file}");
        for l in &loaded.layers {
            volume_from_runs(loaded.bounds, &l.voxels)
                .unwrap_or_else(|e| panic!("{file}, layer {:?}: {e}", l.name));
        }
    }
}

/// A few contents checked without the builders, so a builder edit cannot
/// silently change what the fixtures are compared against.
#[test]
fn fixture_contents_spot_checks() {
    let load = |file: &str| load_rzxl(&fixture_dir().join(file)).unwrap();

    let layers = load("layers.rzxl");
    assert_eq!(layers.layers.len(), 9);
    assert_eq!(layers.bounds, BoundSize::new(24, 16, 24));
    let pyramid = volume_from_runs(layers.bounds, &layers.layers[0].voxels).unwrap();
    assert_eq!(
        pyramid.iter().count(),
        24 * 24 + 22 * 22 + 20 * 20 + 18 * 18 + 16 * 16 + 14 * 14
    );
    assert_eq!(pyramid.get(IVec3::new(12, 5, 12)), 9 + 5 * 8);
    assert!(matches!(
        layers.layers[5].kind,
        LayerKind::Group { collapsed: true }
    ));
    assert_eq!(layers.layers[5].parent, Some(3));
    assert!(matches!(
        layers.layers[7].kind,
        LayerKind::Clone { source: 6, offset } if offset == IVec3::new(-6, -3, -6)
    ));

    let materials = load("materials.rzxl");
    assert_eq!(materials.palette.len(), 16);
    assert_eq!(materials.materials[1].transmission, 0.9);
    assert_eq!(materials.materials[1].ior, 1.45);
    assert_eq!(materials.materials[2].emission_strength, 6.5);
    assert_eq!(materials.materials[3].clearcoat, 0.75);
    assert_eq!(materials.layers[4].material, None);

    let references = load("references.rzxl");
    let planes: Vec<_> = references.references.iter().map(|r| r.plane).collect();
    assert_eq!(planes, [RefPlane::Xy, RefPlane::Xz, RefPlane::Yz]);
    assert!(references.references[1]
        .image
        .bytes()
        .starts_with(&[0xFF, 0xD8]));
    assert!(references.references[0]
        .image
        .bytes()
        .starts_with(b"\x89PNG"));

    let render = load("render.rzxl");
    assert_eq!(render.render.output.kind, OutputKind::Turntable);
    assert_eq!(render.render.light.sky_image, "skies/studio.hdr");
    assert_eq!(render.render.display.shape, VoxelShape::Rounded);
    assert!(!render.render.display.voxel_lines, "version 5: no lines");

    let icon = load_rzxl(&fixture_dir_0_11().join("voxel_icon.rzxl")).unwrap();
    assert!(icon.render.display.voxel_lines && icon.render.display.matte);
    assert_eq!(icon.render.display.floor_grid_spacing, 4);

    let max = load("max_bounds.rzxl");
    let v = volume_from_runs(max.bounds, &max.layers[0].voxels).unwrap();
    assert_eq!(v.get(IVec3::new(255, 255, 255)), 200);
    assert_eq!(v.iter().count(), 8 + 256);
}

/// Every layer kind, shape, reference plane and material field is covered
/// by at least one fixture.
#[test]
fn fixtures_cover_every_feature() {
    let all: Vec<ProjectFile> = all_fixtures().into_iter().map(|(.., p)| p).collect();
    let layers = || all.iter().flat_map(|p| &p.layers);
    assert!(layers().any(|l| matches!(l.kind, LayerKind::Voxels) && !l.voxels.is_empty()));
    assert!(layers().any(|l| matches!(l.kind, LayerKind::Group { collapsed: true })));
    assert!(layers().any(|l| matches!(l.kind, LayerKind::Group { collapsed: false })));
    for shape in ShapeKind::ALL {
        assert!(layers().any(|l| matches!(l.kind, LayerKind::Shape { shape: s, .. } if s == shape)));
    }
    assert!(layers().any(|l| matches!(l.kind, LayerKind::Clone { .. })));
    assert!(layers().any(|l| l.crop.is_some()));
    assert!(layers().any(|l| l.material.is_some()));
    assert!(layers().any(|l| !l.visible && l.locked && l.opacity < 1.0));
    // Nested groups: a group inside a group.
    let p = layers();
    assert!(p.filter(|l| l.kind.is_group()).any(|l| l.parent.is_some()));
    let refs: Vec<&ReferenceImage> = all.iter().flat_map(|p| &p.references).collect();
    for plane in [RefPlane::Xy, RefPlane::Xz, RefPlane::Yz] {
        assert!(refs.iter().any(|r| r.plane == plane));
    }
    let d = Material::new("default");
    let mats: Vec<&Material> = all.iter().flat_map(|p| &p.materials).collect();
    assert!(mats.iter().any(|m| m.metallic != d.metallic));
    assert!(mats.iter().any(|m| m.roughness != d.roughness));
    assert!(mats.iter().any(|m| m.emission != d.emission));
    assert!(mats.iter().any(|m| m.tint != d.tint));
    assert!(mats.iter().any(|m| m.specular != d.specular));
    assert!(mats.iter().any(|m| m.ior != d.ior));
    assert!(mats.iter().any(|m| m.transmission != d.transmission));
    assert!(mats.iter().any(|m| m.clearcoat != d.clearcoat));
    assert!(mats
        .iter()
        .any(|m| m.emission_strength != d.emission_strength));
    // Every render field differs from its default in the 0.10.0 render
    // fixture or, for fields added in version 6, the 0.11.0 one: compare the
    // JSON objects field by field.
    let default = serde_json::to_value(RenderSettings::default()).unwrap();
    let saved = serde_json::to_value(&render().render).unwrap();
    let saved_0_11 = serde_json::to_value(&voxel_icon().render).unwrap();
    for (group, fields) in default.as_object().unwrap() {
        for (field, value) in fields.as_object().unwrap() {
            assert!(
                &saved[group][field] != value || &saved_0_11[group][field] != value,
                "render.{group}.{field}"
            );
        }
    }
    // The builders are valid projects of the current version.
    for p in &all {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, p).unwrap();
        assert_eq!(&buf[4..8], &RZXL_VERSION.to_le_bytes());
        assert_eq!(&read_rzxl(buf.as_slice()).unwrap(), p);
    }
}

/// Writes missing fixture files (see the module docs). Existing files are
/// never overwritten.
#[test]
#[ignore = "writes the fixture files; run once with RIZXEL_WRITE_FIXTURES=1"]
fn write_fixtures() {
    if std::env::var_os("RIZXEL_WRITE_FIXTURES").is_none() {
        return;
    }
    for (path, .., p) in all_fixtures() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if path.exists() {
            eprintln!("keeping {}", path.display());
            continue;
        }
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &p).unwrap();
        std::fs::write(&path, buf).unwrap();
        eprintln!("wrote {}", path.display());
    }
}
