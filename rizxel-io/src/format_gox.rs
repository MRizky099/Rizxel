//! Goxel `.gox` import and export.
//!
//! Layout: `GOX ` + `i32` version (2), then chunks of `type[4]`, `i32`
//! length, data and a 4-byte CRC (written as 0 and ignored, like Goxel).
//! - `BL16`: a 16³ block of RGBA voxels stored as a 64×64 PNG; pixel `i` is
//!   voxel `x + 16y + 256z`.
//! - `LAYR`: block placements (`index, x, y, z, 0`), then a key/value dict
//!   (`name`, `id`, `base_id`, `mat`, `box`, `shape`, `color`, `visible`,
//!   `opacity`, `locked`, `parent_id`, `collapsed`, `material`).
//! - `MATE`: material dict (`name`, `color`, `metallic`, `roughness`,
//!   `emission`).
//! - `IMG `: image dict (`box`, the model bounds).
//! - Other chunks (preview, cameras, lights...) are skipped on import and
//!   not written on export.
//!
//! Goxel is Z-up with true-color voxels and implicit groups: any layer named
//! as another layer's `parent_id` is a group, and it may hold voxels itself.
//!
//! Import:
//! - Colors are reduced to the palette (exact when there are 255 or fewer).
//! - Groups become Rizxel groups. A group's own voxels move to a child layer.
//! - Shape layers stay shapes.
//! - Clones whose transform is a whole-voxel translation stay linked clones.
//!   Other transforms are baked into voxels.
//! - 2D image layers are skipped.
//!
//! Export:
//! - Shapes and linked clones are written natively, and groups become
//!   voxel-less parent layers.
//! - Anything Goxel cannot express is baked into voxels: crop boxes, and
//!   clones of cropped sources.
//! - The model is centered on X/Y and stands on Z = 0. The bounds are
//!   written as the image box.
//! - Material slot 0 is a default material for layers without one.

use std::collections::{HashMap, HashSet};

use glam::{IVec3, UVec3, Vec3};
use rizxel_math::{LayerId, LayerKind, Material, Rgba8, ShapeKind, VoxelBox, VoxelVolume};

use crate::interchange::{
    bounds_for, build_project, dir_from_zup, dir_to_zup, evaluate_layer, from_zup, to_zup,
    zup_size, ImportLayer,
};
use crate::png_io::{decode_rgba, encode_rgba};
use crate::{volume_from_runs, LayerData, ProjectFile, RzxlError};

pub const GOX_EXTENSION: &str = "gox";
const GOX_VERSION: i32 = 2;
const N: i32 = 16;
const MAX_BLOCKS: usize = 1 << 20;
const MAX_LAYERS: usize = 4096;
/// Goxel's own limit on dict keys and values.
const MAX_DICT_VALUE: usize = 255;

fn invalid(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("Goxel: {}", msg.into()))
}

// ------------------------------------------------------------------ reading

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], RzxlError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| invalid("unexpected end of data"))?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn i32(&mut self) -> Result<i32, RzxlError> {
        let b = self.bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn len(&mut self, max: usize) -> Result<usize, RzxlError> {
        let n = self.i32()?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= max)
            .ok_or_else(|| invalid(format!("bad length {n}")))
    }

    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Reads the trailing key/value dict of a chunk.
    fn dict(&mut self) -> Result<HashMap<String, &'a [u8]>, RzxlError> {
        let mut d = HashMap::new();
        while !self.done() {
            let k = self.len(MAX_DICT_VALUE)?;
            if k == 0 {
                break;
            }
            let key = String::from_utf8_lossy(self.bytes(k)?).into_owned();
            let v = self.len(MAX_DICT_VALUE)?;
            d.insert(key, self.bytes(v)?);
        }
        Ok(d)
    }
}

fn f32s<const K: usize>(v: &[u8]) -> Option<[f32; K]> {
    if v.len() != K * 4 {
        return None;
    }
    let mut out = [0.0; K];
    for (i, c) in v.chunks_exact(4).enumerate() {
        out[i] = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
    out.iter().all(|x| x.is_finite()).then_some(out)
}

fn i32_of(v: &[u8]) -> Option<i32> {
    (v.len() == 4).then(|| i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn bool_of(v: &[u8]) -> Option<bool> {
    v.first().map(|&b| b != 0)
}

/// Goxel `box`: column 3 is the center, columns 0..2 the half axes.
fn box_cells(b: &[f32; 16]) -> Option<(IVec3, IVec3)> {
    if b[15] == 0.0 {
        return None; // null box
    }
    let center = Vec3::new(b[12], b[13], b[14]);
    let half = Vec3::new(b[0], b[5], b[10]).abs();
    let lo = (center - half).round().as_ivec3();
    let hi = (center + half).round().as_ivec3();
    lo.cmplt(hi).all().then_some((lo, hi - IVec3::ONE))
}

struct GLayer {
    id: i32,
    name: String,
    cells: HashMap<IVec3, Rgba8>,
    mat: Option<[f32; 16]>,
    base_id: i32,
    shape_box: Option<[f32; 16]>,
    shape: Option<ShapeKind>,
    color: Rgba8,
    visible: bool,
    opacity: f32,
    locked: bool,
    parent_id: i32,
    collapsed: bool,
    material: i32,
    image: bool,
}

/// What a Goxel layer becomes before final ids and colors are known.
enum Geometry {
    None,
    Cells(HashMap<IVec3, Rgba8>),
    Shape(ShapeKind, IVec3, IVec3, Rgba8),
    Clone(i32, IVec3),
}

/// Parses a `.gox` file into a project named `name`.
pub fn read_gox(bytes: &[u8], name: &str) -> Result<ProjectFile, RzxlError> {
    let mut c = Cursor {
        data: bytes,
        pos: 0,
    };
    if c.bytes(4)? != b"GOX " {
        return Err(invalid("not a Goxel file (bad magic)"));
    }
    let version = c.i32()?;
    if !(1..=GOX_VERSION).contains(&version) {
        return Err(invalid(format!("unsupported version {version}")));
    }

    let mut blocks: Vec<[Rgba8; 4096]> = Vec::new();
    let mut layers: Vec<GLayer> = Vec::new();
    let mut materials: Vec<Material> = Vec::new();
    let mut image_box: Option<(IVec3, IVec3)> = None;
    while c.pos + 8 <= bytes.len() {
        let kind: [u8; 4] = c.bytes(4)?.try_into().map_err(|_| invalid("chunk type"))?;
        let len = c.len(bytes.len())?;
        let data = c.bytes(len)?;
        c.bytes(4)?; // CRC (unused)
        let mut d = Cursor { data, pos: 0 };
        match &kind {
            b"BL16" => {
                if blocks.len() >= MAX_BLOCKS {
                    return Err(invalid("too many blocks"));
                }
                let img = decode_rgba(data)?;
                if (img.width, img.height) != (64, 64) {
                    return Err(invalid("block image is not 64×64"));
                }
                let mut b = [Rgba8::new(0, 0, 0, 0); 4096];
                for (i, px) in img.pixels.chunks_exact(4).enumerate() {
                    b[i] = Rgba8::new(px[0], px[1], px[2], px[3]);
                }
                blocks.push(b);
            }
            b"IMG " => {
                let dict = d.dict()?;
                image_box = dict
                    .get("box")
                    .and_then(|v| f32s::<16>(v))
                    .and_then(|b| box_cells(&b));
            }
            b"MATE" => {
                let dict = d.dict()?;
                let unit = |v: f32| v.clamp(0.0, 1.0);
                let mut m = Material::new(
                    dict.get("name")
                        .map(|v| String::from_utf8_lossy(v).into_owned())
                        .unwrap_or_else(|| format!("Material {}", materials.len() + 1)),
                );
                if let Some(c) = dict.get("color").and_then(|v| f32s::<4>(v)) {
                    m.tint = c.map(unit);
                }
                if let Some([v]) = dict.get("metallic").and_then(|v| f32s::<1>(v)) {
                    m.metallic = unit(v);
                }
                if let Some([v]) = dict.get("roughness").and_then(|v| f32s::<1>(v)) {
                    m.roughness = unit(v);
                }
                if let Some(e) = dict.get("emission").and_then(|v| f32s::<3>(v)) {
                    m.emission = e.map(unit);
                }
                if materials.len() < crate::MAX_MATERIALS {
                    materials.push(m);
                }
            }
            b"LAYR" => {
                if layers.len() >= MAX_LAYERS {
                    return Err(invalid("too many layers"));
                }
                let n = d.len(MAX_BLOCKS)?;
                let mut cells = HashMap::new();
                for _ in 0..n {
                    let index = d.len(MAX_BLOCKS)?;
                    let mut pos = IVec3::new(d.i32()?, d.i32()?, d.i32()?);
                    if version == 1 {
                        pos -= IVec3::splat(8);
                    }
                    d.i32()?;
                    if pos.abs().cmpgt(IVec3::splat(1 << 20)).any() {
                        return Err(invalid("block position out of range"));
                    }
                    let block = blocks
                        .get(index)
                        .ok_or_else(|| invalid(format!("layer uses missing block {index}")))?;
                    for (i, &rgba) in block.iter().enumerate() {
                        if rgba.a > 0 {
                            let l = IVec3::new(i as i32 % N, i as i32 / N % N, i as i32 / (N * N));
                            cells.insert(pos + l, rgba);
                        }
                    }
                }
                let dict = d.dict()?;
                let get_i32 = |k: &str| dict.get(k).and_then(|v| i32_of(v));
                let get_bool = |k: &str| dict.get(k).and_then(|v| bool_of(v));
                layers.push(GLayer {
                    id: get_i32("id").unwrap_or(0),
                    name: dict
                        .get("name")
                        .map(|v| String::from_utf8_lossy(v).trim_end_matches('\0').to_owned())
                        .unwrap_or_default(),
                    cells,
                    mat: dict.get("mat").and_then(|v| f32s::<16>(v)),
                    base_id: get_i32("base_id").unwrap_or(0),
                    shape_box: dict.get("box").and_then(|v| f32s::<16>(v)),
                    shape: dict.get("shape").and_then(|v| match &**v {
                        b"sphere" | b"spray" => Some(ShapeKind::Sphere),
                        b"cube" => Some(ShapeKind::Cube),
                        b"cylinder" => Some(ShapeKind::Cylinder),
                        _ => None,
                    }),
                    color: dict
                        .get("color")
                        .filter(|v| v.len() == 4)
                        .map(|v| Rgba8::new(v[0], v[1], v[2], v[3]))
                        .unwrap_or(Rgba8::WHITE),
                    visible: get_bool("visible").unwrap_or(true),
                    opacity: dict
                        .get("opacity")
                        .and_then(|v| f32s::<1>(v))
                        .map(|[o]| o.clamp(0.0, 1.0))
                        .unwrap_or(1.0),
                    locked: get_bool("locked").unwrap_or(false),
                    parent_id: get_i32("parent_id").unwrap_or(0),
                    collapsed: get_bool("collapsed").unwrap_or(false),
                    material: get_i32("material").unwrap_or(-1),
                    image: dict.contains_key("img-path"),
                });
            }
            _ => {}
        }
    }
    if layers.is_empty() {
        return Err(invalid("the file contains no layers"));
    }
    convert(layers, materials, image_box, name)
}

fn convert(
    mut layers: Vec<GLayer>,
    materials: Vec<Material>,
    image_box: Option<(IVec3, IVec3)>,
    name: &str,
) -> Result<ProjectFile, RzxlError> {
    // Give id-less (old) layers unique ids.
    // Old files have no ids; damaged ones may repeat them.
    let mut next = layers.iter().map(|l| l.id).max().unwrap_or(0).max(0) + 1;
    let mut seen: HashSet<i32> = HashSet::new();
    for l in layers.iter_mut() {
        if l.id <= 0 || !seen.insert(l.id) {
            l.id = next;
            next += 1;
            seen.insert(l.id);
        }
    }
    let by_id: HashMap<i32, usize> = layers.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
    let parents: HashSet<i32> = layers
        .iter()
        .map(|l| l.parent_id)
        .filter(|p| by_id.contains_key(p))
        .collect();

    // Geometry per layer, in Goxel cells.
    let geometry: Vec<Geometry> = (0..layers.len())
        .map(|i| geometry_of(&layers, &by_id, i, 0))
        .collect();

    // Scene extent over everything that will hold voxels.
    let (mut lo, mut hi) = (IVec3::splat(i32::MAX), IVec3::splat(i32::MIN));
    let mut grow = |p: IVec3| {
        lo = lo.min(p);
        hi = hi.max(p);
    };
    if let Some((a, b)) = image_box {
        grow(a);
        grow(b);
    }
    for (i, g) in geometry.iter().enumerate() {
        match g {
            Geometry::Cells(cells) => cells.keys().for_each(|&p| grow(p)),
            Geometry::Shape(_, a, b, _) => {
                grow(*a);
                grow(*b);
            }
            // The image box is Goxel's equivalent of the bounds: moved clone
            // geometry only widens the scene when there is no image box
            // (Rizxel clips clones at the bounds, like stored voxels).
            Geometry::Clone(base, t) if image_box.is_none() => {
                if let Some(&bi) = by_id.get(base) {
                    match &geometry[bi] {
                        Geometry::Cells(cells) => cells.keys().for_each(|&p| grow(p + *t)),
                        Geometry::Shape(_, a, b, _) => {
                            grow(*a + *t);
                            grow(*b + *t);
                        }
                        _ => {}
                    }
                }
                let _ = i;
            }
            Geometry::Clone(..) | Geometry::None => {}
        }
    }
    if lo.cmpgt(hi).any() {
        // Nothing but groups: a minimal 1³ scene.
        lo = IVec3::ZERO;
        hi = IVec3::ZERO;
    }
    let zsize = hi - lo + IVec3::ONE;
    let bounds = bounds_for(
        UVec3::new(zsize.x as u32, zsize.z as u32, zsize.y as u32),
        "The Goxel model",
    )?;
    let cell = |g: IVec3| from_zup(zsize, g - lo);

    // Palette from every voxel and shape color, in a stable order.
    let mut colors: Vec<(Rgba8, u64)> = Vec::new();
    let mut index: HashMap<Rgba8, usize> = HashMap::new();
    let mut count = |c: Rgba8| match index.get(&c) {
        Some(&i) => colors[i].1 += 1,
        None => {
            index.insert(c, colors.len());
            colors.push((c, 1));
        }
    };
    for g in &geometry {
        match g {
            Geometry::Cells(cells) => {
                let mut sorted: Vec<_> = cells.iter().collect();
                sorted.sort_by_key(|(p, _)| (p.z, p.y, p.x));
                sorted.into_iter().for_each(|(_, &c)| count(c));
            }
            Geometry::Shape(.., c) => count(*c),
            _ => {}
        }
    }
    if colors.is_empty() {
        colors.push((Rgba8::WHITE, 1));
    }
    let map = rizxel_engine::build_color_map(&colors);

    // Rizxel layers. Ids are kept; a group's own voxels get a fresh id.
    let mut next_id = layers.iter().map(|l| l.id).max().unwrap_or(0) as LayerId + 1;
    let mut voxel_child: HashMap<i32, LayerId> = HashMap::new();
    let mut out: Vec<ImportLayer> = Vec::new();
    for (i, l) in layers.iter().enumerate() {
        if l.image {
            continue;
        }
        let is_group = parents.contains(&l.id);
        let parent = (l.parent_id != 0 && by_id.contains_key(&l.parent_id) && l.parent_id != l.id)
            .then_some(l.parent_id as LayerId);
        let material = (l.material >= 0 && (l.material as usize) < materials.len())
            .then_some(l.material as u32);
        let base = LayerData {
            visible: l.visible,
            locked: l.locked,
            opacity: l.opacity,
            parent,
            material,
            ..LayerData::new(l.id as LayerId, l.name.clone())
        };
        let (geom_id, geom_parent) = if is_group {
            out.push(ImportLayer {
                data: LayerData {
                    kind: LayerKind::Group {
                        collapsed: l.collapsed,
                    },
                    material: None,
                    ..base.clone()
                },
                volume: None,
            });
            if matches!(geometry[i], Geometry::None) {
                continue;
            }
            let id = next_id;
            next_id += 1;
            voxel_child.insert(l.id, id);
            (id, Some(l.id as LayerId))
        } else {
            (l.id as LayerId, parent)
        };
        let mut data = LayerData {
            id: geom_id,
            parent: geom_parent,
            ..base
        };
        if is_group {
            data.name = format!("{} voxels", l.name);
        }
        let volume = match &geometry[i] {
            Geometry::None => Some(VoxelVolume::new(bounds)),
            Geometry::Cells(cells) => {
                let mut v = VoxelVolume::new(bounds);
                for (&p, &c) in cells {
                    v.set(cell(p), map.value(c));
                }
                Some(v)
            }
            Geometry::Shape(shape, a, b, c) => {
                let (ra, rb) = (cell(*a), cell(*b));
                data.kind = LayerKind::Shape {
                    shape: *shape,
                    area: VoxelBox::from_corners(ra, rb),
                    color: map.value(*c),
                };
                None
            }
            Geometry::Clone(base_id, t) => {
                data.kind = LayerKind::Clone {
                    // Filled in below once every group child id is known.
                    source: *base_id as LayerId,
                    offset: dir_from_zup(*t),
                };
                None
            }
        };
        out.push(ImportLayer { data, volume });
    }
    // Clones of groups point at the group's voxel child.
    for l in &mut out {
        if let LayerKind::Clone { source, .. } = &mut l.data.kind {
            if let Some(&child) = voxel_child.get(&(*source as i32)) {
                *source = child;
            }
        }
    }
    break_parent_cycles(&mut out);
    build_project(name, bounds, map.palette, materials, out)
}

/// Resolves what layer `i` contributes. Clones that are not a whole-voxel
/// translation of a voxel or shape layer are baked into cells.
fn geometry_of(layers: &[GLayer], by_id: &HashMap<i32, usize>, i: usize, depth: usize) -> Geometry {
    let l = &layers[i];
    if let (Some(shape), Some(b)) = (l.shape, l.shape_box) {
        if let Some((a, b)) = box_cells(&b) {
            return Geometry::Shape(shape, a, b, l.color);
        }
    }
    if l.base_id != 0 && depth < 16 {
        let Some(&bi) = by_id.get(&l.base_id).filter(|&&bi| bi != i) else {
            return Geometry::None;
        };
        let m = l.mat.unwrap_or(IDENTITY);
        let linear_identity =
            (0..3).all(|c| (0..3).all(|r| (m[c * 4 + r] - f32::from(c == r)).abs() < 1e-4));
        let t = Vec3::new(m[12], m[13], m[14]);
        let integral = (t - t.round()).abs().max_element() < 1e-4;
        let base = geometry_of(layers, by_id, bi, depth + 1);
        let base_is_source = matches!(base, Geometry::Cells(_) | Geometry::Shape(..));
        if linear_identity && integral && base_is_source && !layers[bi].image {
            return Geometry::Clone(l.base_id, t.round().as_ivec3());
        }
        // Bake: move each base cell center through the matrix.
        let base_cells: HashMap<IVec3, Rgba8> = match base {
            Geometry::Cells(c) => c,
            Geometry::Shape(shape, a, b, color) => shape_cells(shape, a, b, color),
            _ => HashMap::new(),
        };
        let cells = base_cells
            .into_iter()
            .map(|(p, c)| {
                let q = p.as_vec3() + Vec3::splat(0.5);
                let w = Vec3::new(
                    m[0] * q.x + m[4] * q.y + m[8] * q.z + m[12],
                    m[1] * q.x + m[5] * q.y + m[9] * q.z + m[13],
                    m[2] * q.x + m[6] * q.y + m[10] * q.z + m[14],
                );
                (w.floor().as_ivec3(), c)
            })
            .collect();
        return Geometry::Cells(cells);
    }
    if l.cells.is_empty() {
        Geometry::None
    } else {
        Geometry::Cells(l.cells.clone())
    }
}

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Goxel-space cells of a shape spanning cells `a..=b`.
fn shape_cells(shape: ShapeKind, a: IVec3, b: IVec3, color: Rgba8) -> HashMap<IVec3, Rgba8> {
    let size = b - a + IVec3::ONE;
    if size.cmpgt(IVec3::splat(1024)).any() {
        return HashMap::new();
    }
    let center = (a.as_vec3() + (b + IVec3::ONE).as_vec3()) * 0.5;
    let half = size.as_vec3() * 0.5;
    let mut out = HashMap::new();
    for z in a.z..=b.z {
        for y in a.y..=b.y {
            for x in a.x..=b.x {
                let p = IVec3::new(x, y, z);
                let d = (p.as_vec3() + Vec3::splat(0.5) - center) / half;
                let inside = match shape {
                    ShapeKind::Cube => true,
                    ShapeKind::Sphere => d.length_squared() <= 1.0,
                    // Goxel cylinders stand along Z (up).
                    ShapeKind::Cylinder => d.x * d.x + d.y * d.y <= 1.0,
                };
                if inside {
                    out.insert(p, color);
                }
            }
        }
    }
    out
}

/// Drops `parent` links that would form a cycle or nest too deep.
fn break_parent_cycles(layers: &mut [ImportLayer]) {
    let parents: HashMap<LayerId, Option<LayerId>> =
        layers.iter().map(|l| (l.data.id, l.data.parent)).collect();
    for l in layers.iter_mut() {
        let mut seen = HashSet::from([l.data.id]);
        let mut p = l.data.parent;
        let mut depth = 0;
        while let Some(id) = p {
            depth += 1;
            if !seen.insert(id) || depth > crate::MAX_GROUP_DEPTH {
                l.data.parent = None;
                break;
            }
            p = parents.get(&id).copied().flatten();
        }
    }
}

// ------------------------------------------------------------------ writing

struct Out {
    bytes: Vec<u8>,
}

impl Out {
    fn i32(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn chunk(&mut self, kind: &[u8; 4], data: &[u8]) {
        self.bytes.extend_from_slice(kind);
        self.i32(data.len() as i32);
        self.bytes.extend_from_slice(data);
        self.i32(0); // CRC, unused by Goxel
    }
}

#[derive(Default)]
struct Dict {
    bytes: Vec<u8>,
}

impl Dict {
    fn raw(&mut self, key: &str, value: &[u8]) {
        let value = &value[..value.len().min(MAX_DICT_VALUE)];
        self.bytes
            .extend_from_slice(&(key.len() as i32).to_le_bytes());
        self.bytes.extend_from_slice(key.as_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as i32).to_le_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn str(&mut self, key: &str, s: &str) {
        // Keep within Goxel's limit without splitting a UTF-8 character.
        let mut end = s.len().min(MAX_DICT_VALUE);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.raw(key, &s.as_bytes()[..end]);
    }

    fn i32(&mut self, key: &str, v: i32) {
        self.raw(key, &v.to_le_bytes());
    }

    fn bool(&mut self, key: &str, v: bool) {
        self.raw(key, &[v as u8]);
    }

    fn f32s(&mut self, key: &str, v: &[f32]) {
        let b: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.raw(key, &b);
    }
}

fn box_matrix(lo: Vec3, hi: Vec3) -> [f32; 16] {
    let c = (lo + hi) * 0.5;
    let h = (hi - lo) * 0.5;
    [
        h.x, 0.0, 0.0, 0.0, 0.0, h.y, 0.0, 0.0, 0.0, 0.0, h.z, 0.0, c.x, c.y, c.z, 1.0,
    ]
}

/// Encodes a project as a `.gox` file.
pub fn write_gox(project: &ProjectFile) -> Result<Vec<u8>, RzxlError> {
    project.validate()?;
    let bounds = project.bounds;
    let zsize = zup_size(bounds);
    // Goxel frame: X/Y centered, standing on Z = 0.
    let shift = IVec3::new(zsize.x / 2, zsize.y / 2, 0);
    let to_gox = |v: IVec3| to_zup(bounds, v) - shift;
    let color = |v: u8| {
        let c = project.palette.get(v as usize).unwrap_or(Rgba8::WHITE);
        Rgba8::new(c.r, c.g, c.b, 255)
    };

    let volumes: Vec<Option<VoxelVolume>> = project
        .layers
        .iter()
        .map(|l| match l.kind {
            LayerKind::Voxels => volume_from_runs(bounds, &l.voxels).map(Some),
            _ => Ok(None),
        })
        .collect::<Result<_, _>>()?;
    let has_crop = |id: LayerId| {
        project
            .layers
            .iter()
            .find(|l| l.id == id)
            .is_some_and(|l| l.crop.is_some())
    };

    // Per layer: either a native kind (shape/clone/group) or baked blocks.
    let mut block_ids: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut block_pngs: Vec<Vec<u8>> = Vec::new();
    let mut layer_chunks: Vec<Vec<u8>> = Vec::new();
    for (i, l) in project.layers.iter().enumerate() {
        let mut dict = Dict::default();
        let mut placements: Vec<(usize, IVec3)> = Vec::new();
        let native = match l.kind {
            LayerKind::Group { .. } => true,
            LayerKind::Shape { .. } => l.crop.is_none(),
            LayerKind::Clone { source, .. } => l.crop.is_none() && !has_crop(source),
            LayerKind::Voxels => false,
        };
        let mut mat = IDENTITY;
        match l.kind {
            LayerKind::Shape {
                shape,
                area,
                color: c,
            } if native => {
                // Cells min..max-1 in Rizxel -> Goxel corner box.
                let a = to_gox(area.min);
                let b = to_gox(area.max - IVec3::ONE);
                let (lo, hi) = (a.min(b).as_vec3(), (a.max(b) + IVec3::ONE).as_vec3());
                dict.f32s("box", &box_matrix(lo, hi));
                dict.str(
                    "shape",
                    match shape {
                        ShapeKind::Cube => "cube",
                        ShapeKind::Sphere => "sphere",
                        ShapeKind::Cylinder => "cylinder",
                    },
                );
                let c = color(c);
                dict.raw("color", &[c.r, c.g, c.b, c.a]);
            }
            LayerKind::Clone { source, offset } if native => {
                let t = dir_to_zup(offset).as_vec3();
                mat[12] = t.x;
                mat[13] = t.y;
                mat[14] = t.z;
                dict.i32("base_id", source as i32);
            }
            LayerKind::Group { .. } => {}
            _ => {
                if let Some(v) = evaluate_layer(project, &volumes, i) {
                    placements =
                        encode_blocks(&v, &to_gox, &color, &mut block_ids, &mut block_pngs)?;
                }
            }
        }
        let mut data = Vec::new();
        data.extend_from_slice(&(placements.len() as i32).to_le_bytes());
        for (index, pos) in &placements {
            for v in [*index as i32, pos.x, pos.y, pos.z, 0] {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        dict.str("name", &l.name);
        dict.f32s("mat", &mat);
        dict.i32("id", l.id as i32);
        if !matches!(l.kind, LayerKind::Clone { .. }) || !native {
            dict.i32("base_id", 0);
        }
        dict.i32("material", l.material.map_or(0, |m| m as i32 + 1));
        dict.bool("visible", l.visible);
        dict.f32s("opacity", &[l.opacity]);
        dict.i32("parent_id", l.parent.map_or(0, |p| p as i32));
        dict.bool(
            "collapsed",
            matches!(l.kind, LayerKind::Group { collapsed: true }),
        );
        dict.bool("locked", l.locked);
        data.extend_from_slice(&dict.bytes);
        layer_chunks.push(data);
    }

    let mut out = Out { bytes: Vec::new() };
    out.bytes.extend_from_slice(b"GOX ");
    out.i32(GOX_VERSION);
    let mut img = Dict::default();
    let lo = (-shift).as_vec3();
    img.f32s("box", &box_matrix(lo, lo + zsize.as_vec3()));
    out.chunk(b"IMG ", &img.bytes);
    for png in &block_pngs {
        out.chunk(b"BL16", png);
    }
    // Slot 0 is the default material for layers without one.
    for m in std::iter::once(&Material::default()).chain(&project.materials) {
        let mut d = Dict::default();
        d.str("name", &m.name);
        d.f32s("color", &m.tint);
        d.f32s("metallic", &[m.metallic]);
        d.f32s("roughness", &[m.roughness]);
        d.f32s("emission", &m.emission);
        out.chunk(b"MATE", &d.bytes);
    }
    for data in &layer_chunks {
        out.chunk(b"LAYR", data);
    }
    Ok(out.bytes)
}

/// Splits a volume into 16³ Goxel blocks (deduplicated by content).
fn encode_blocks(
    volume: &VoxelVolume,
    to_gox: &impl Fn(IVec3) -> IVec3,
    color: &impl Fn(u8) -> Rgba8,
    ids: &mut HashMap<Vec<u8>, usize>,
    pngs: &mut Vec<Vec<u8>>,
) -> Result<Vec<(usize, IVec3)>, RzxlError> {
    let mut tiles: HashMap<IVec3, Vec<u8>> = HashMap::new();
    for (p, v) in volume.iter() {
        let g = to_gox(p);
        let origin = g.div_euclid(IVec3::splat(N)) * N;
        let l = g - origin;
        let i = ((l.x + l.y * N + l.z * N * N) * 4) as usize;
        let tile = tiles.entry(origin).or_insert_with(|| vec![0; 4096 * 4]);
        let c = color(v);
        tile[i..i + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    let mut origins: Vec<IVec3> = tiles.keys().copied().collect();
    origins.sort_by_key(|o| (o.z, o.y, o.x));
    let mut out = Vec::with_capacity(origins.len());
    for o in origins {
        let pixels = tiles.remove(&o).unwrap_or_default();
        let index = match ids.get(&pixels) {
            Some(&i) => i,
            None => {
                pngs.push(encode_rgba(64, 64, &pixels)?);
                ids.insert(pixels, pngs.len() - 1);
                pngs.len() - 1
            }
        };
        out.push((index, o));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::evaluate_layer;
    use rizxel_math::{BoundSize, Palette};

    fn project() -> ProjectFile {
        let bounds = BoundSize::new(20, 18, 24);
        let mut v = VoxelVolume::new(bounds);
        for x in 0..20 {
            v.set(IVec3::new(x, 0, 3), 9);
        }
        v.set(IVec3::new(5, 17, 23), 40);
        v.set(IVec3::new(17, 6, 0), 12);
        let mut cropped = VoxelVolume::new(bounds);
        cropped.set(IVec3::new(1, 1, 1), 3);
        cropped.set(IVec3::new(10, 1, 1), 3);
        let layers = vec![
            ImportLayer {
                data: LayerData {
                    kind: LayerKind::Group { collapsed: true },
                    ..LayerData::new(1, "Group")
                },
                volume: None,
            },
            ImportLayer {
                data: LayerData {
                    parent: Some(1),
                    opacity: 0.5,
                    material: Some(0),
                    ..LayerData::new(2, "Base")
                },
                volume: Some(v),
            },
            ImportLayer {
                data: LayerData {
                    kind: LayerKind::Clone {
                        source: 2,
                        offset: IVec3::new(0, 2, -1),
                    },
                    visible: false,
                    ..LayerData::new(3, "Linked")
                },
                volume: None,
            },
            ImportLayer {
                data: LayerData {
                    kind: LayerKind::Shape {
                        shape: ShapeKind::Cylinder,
                        area: VoxelBox::from_corners(IVec3::new(2, 3, 4), IVec3::new(9, 12, 11)),
                        color: 20,
                    },
                    locked: true,
                    ..LayerData::new(4, "Pillar")
                },
                volume: None,
            },
            ImportLayer {
                data: LayerData {
                    crop: Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::splat(5))),
                    ..LayerData::new(5, "Cropped")
                },
                volume: Some(cropped),
            },
        ];
        let materials = vec![Material {
            emission: [0.5, 0.25, 0.0],
            ..Material::new("Glow")
        }];
        build_project("t", bounds, Palette::default(), materials, layers).unwrap()
    }

    fn geometry(p: &ProjectFile) -> Vec<Option<VoxelVolume>> {
        let volumes: Vec<Option<VoxelVolume>> = p
            .layers
            .iter()
            .map(|l| volume_from_runs(p.bounds, &l.voxels).ok())
            .collect();
        (0..p.layers.len())
            .map(|i| evaluate_layer(p, &volumes, i))
            .collect()
    }

    #[test]
    fn export_import_roundtrip_keeps_structure_and_geometry() {
        let src = project();
        let bytes = write_gox(&src).unwrap();
        assert_eq!(&bytes[0..4], b"GOX ");
        let back = read_gox(&bytes, "t").unwrap();
        assert_eq!(back.bounds, src.bounds, "image box restores the bounds");
        let kinds: Vec<_> = back
            .layers
            .iter()
            .map(|l| (l.id, l.name.as_str(), l.kind))
            .collect();
        assert_eq!(kinds.len(), 5, "{kinds:?}");
        assert!(back.layers[0].kind.is_group());
        assert_eq!(back.layers[1].parent, Some(1));
        assert_eq!(back.layers[1].opacity, 0.5);
        assert_eq!(back.layers[1].material, Some(1), "after the default slot");
        assert_eq!(back.materials[1].name, "Glow");
        assert!(
            matches!(back.layers[2].kind, LayerKind::Clone { source: 2, offset } if offset == IVec3::new(0, 2, -1))
        );
        assert!(!back.layers[2].visible);
        assert!(matches!(
            back.layers[3].kind,
            LayerKind::Shape {
                shape: ShapeKind::Cylinder,
                ..
            }
        ));
        assert!(back.layers[3].locked);
        assert_eq!(back.layers[4].crop, None, "crop is baked");
        // Same voxels and colors in every layer (palette indices may differ:
        // Goxel stores true colors).
        let rgb = |p: &ProjectFile, v: &Option<VoxelVolume>| -> HashMap<IVec3, Rgba8> {
            v.iter()
                .flat_map(|v| v.iter())
                .map(|(pos, c)| {
                    let c = p.palette.get(c as usize).unwrap();
                    (pos, Rgba8::rgb(c.r, c.g, c.b))
                })
                .collect()
        };
        let (a, b) = (geometry(&src), geometry(&back));
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert_eq!(rgb(&src, x), rgb(&back, y), "layer {i}");
        }
    }

    #[test]
    fn reads_users_goxel_file_when_present() {
        let path = std::path::Path::new("F:/Bricks_n_Voxel/Cobacoba/coba_goxel.gox");
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let p = read_gox(&bytes, "coba_goxel").unwrap();
        let voxels: u32 = p.layers.iter().flat_map(|l| &l.voxels).map(|r| r.len).sum();
        assert!(voxels > 0);
    }

    #[test]
    fn hostile_files_fail_cleanly() {
        assert!(read_gox(b"GOX \x02\0\0\0", "x").is_err(), "no layers");
        assert!(read_gox(b"VOX \x02\0\0\0", "x").is_err());
        let mut bytes = write_gox(&project()).unwrap();
        bytes.truncate(bytes.len() - 7);
        let _ = read_gox(&bytes, "x");
        // A layer pointing at a block that does not exist.
        let mut out = Out {
            bytes: b"GOX \x02\0\0\0".to_vec(),
        };
        let mut data = Vec::new();
        for v in [1i32, 5, 0, 0, 0, 0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        out.chunk(b"LAYR", &data);
        assert!(read_gox(&out.bytes, "x")
            .unwrap_err()
            .to_string()
            .contains("missing block"));
    }
}
