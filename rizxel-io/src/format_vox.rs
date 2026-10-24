//! MagicaVoxel `.vox` import and export.
//!
//! Layout (RIFF-like, little-endian): `VOX ` + version, then a `MAIN` chunk
//! whose children are `SIZE`/`XYZI` model pairs, the scene graph (`nTRN`
//! transforms, `nGRP` groups, `nSHP` shapes), `LAYR` layers and the `RGBA`
//! palette. Chunk headers are `id, content bytes, children bytes`.
//!
//! Import:
//! - Every shape instance in the scene graph becomes a Rizxel layer, placed
//!   with its accumulated translation and rotation:
//!   `world = R * (p - size / 2) + t`, with integer halves as in
//!   MagicaVoxel.
//! - Named or nested groups become Rizxel groups.
//! - Later instances of an already-placed model with the same rotation
//!   become **linked clones**.
//! - Hidden nodes and hidden MagicaVoxel layers import hidden.
//! - The palette maps one to one: color index `i` is Rizxel palette slot `i`.
//! - Files without a scene graph place all models at the origin.
//!
//! Export:
//! - One model per non-empty Rizxel layer (shapes, clones and crops are
//!   baked), all the size of the bounds.
//! - Each model gets a transform carrying the layer's name and hidden flag.
//! - Rizxel groups become named `nGRP` nodes, nested as in the Layers
//!   panel, so they come back as groups on import.
//! - The palette is written as the `RGBA` chunk.
//! - Rizxel Y-up is converted to MagicaVoxel Z-up with a proper rotation,
//!   so nothing is mirrored.

use std::collections::{HashMap, HashSet};

use glam::{IVec3, UVec3};
use rizxel_math::{LayerId, LayerKind, Palette, Rgba8, VoxelVolume};

use crate::interchange::{bounds_for, build_project, dir_from_zup, from_zup, to_zup, zup_size};
use crate::interchange::{ExportScene, ImportLayer};
use crate::{LayerData, ProjectFile, RzxlError};

pub const VOX_EXTENSION: &str = "vox";
/// Version written by this exporter (MagicaVoxel 0.99 scene graph).
const VOX_VERSION: i32 = 200;

const MAX_MODELS: usize = 4096;
const MAX_NODES: usize = 65536;
const MAX_DICT_ENTRIES: usize = 1024;
const MAX_STRING: usize = 65536;
const MAX_SCENE_DEPTH: usize = 64;

/// MagicaVoxel's default palette (color index `i` = entry `i - 1`). Used for
/// files without an `RGBA` chunk and to fill unused export slots.
pub fn default_palette() -> [Rgba8; 256] {
    let mut out = [Rgba8::new(0, 0, 0, 0); 256];
    let levels = [0xff, 0xcc, 0x99, 0x66, 0x33, 0x00];
    let mut i = 0;
    for r in levels {
        for g in levels {
            for b in levels {
                if i < 215 {
                    out[i] = Rgba8::rgb(r, g, b);
                    i += 1;
                }
            }
        }
    }
    let ramp = [0xee, 0xdd, 0xbb, 0xaa, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];
    for channel in 0..4 {
        for v in ramp {
            out[i] = match channel {
                0 => Rgba8::rgb(v, 0, 0),
                1 => Rgba8::rgb(0, v, 0),
                2 => Rgba8::rgb(0, 0, v),
                _ => Rgba8::rgb(v, v, v),
            };
            i += 1;
        }
    }
    out
}

/// The 256 MagicaVoxel palette entries for a Rizxel palette: entry `j` is
/// Rizxel slot `j + 1` (MagicaVoxel's default where the palette is
/// shorter), and the last entry, which no voxel can use, is transparent.
pub fn export_palette(palette: &Palette) -> [Rgba8; 256] {
    let mut out = default_palette();
    for (j, slot) in out.iter_mut().enumerate().take(255) {
        if let Some(c) = palette.get(j + 1) {
            *slot = c;
        }
    }
    out[255] = Rgba8::new(0, 0, 0, 0);
    out
}

fn invalid(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("MagicaVoxel: {}", msg.into()))
}

// ------------------------------------------------------------------ reading

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
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

    fn count(&mut self, max: usize, what: &str) -> Result<usize, RzxlError> {
        let n = self.i32()?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= max)
            .ok_or_else(|| invalid(format!("bad {what} count {n}")))
    }

    fn string(&mut self) -> Result<String, RzxlError> {
        let n = self.count(MAX_STRING, "string length")?;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }

    fn dict(&mut self) -> Result<HashMap<String, String>, RzxlError> {
        let n = self.count(MAX_DICT_ENTRIES, "dictionary")?;
        let mut d = HashMap::new();
        for _ in 0..n {
            let k = self.string()?;
            let v = self.string()?;
            d.insert(k, v);
        }
        Ok(d)
    }
}

enum Node {
    Transform {
        attrs: HashMap<String, String>,
        child: i32,
        layer: i32,
        frame: HashMap<String, String>,
    },
    Group {
        children: Vec<i32>,
    },
    Shape {
        models: Vec<i32>,
    },
}

struct Model {
    size: IVec3,
    voxels: Vec<[u8; 4]>,
}

/// 3×3 signed permutation (MagicaVoxel `_r`), rows applied to a vector.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Rot([IVec3; 3]);

impl Rot {
    const IDENTITY: Rot = Rot([IVec3::X, IVec3::Y, IVec3::Z]);

    fn parse(s: &str) -> Option<Rot> {
        let r: u8 = s.trim().parse().ok()?;
        let (i0, i1) = ((r & 3) as usize, ((r >> 2) & 3) as usize);
        if i0 > 2 || i1 > 2 || i0 == i1 {
            return None;
        }
        let i2 = 3 - i0 - i1;
        let row = |i: usize, bit: u8| {
            let mut v = IVec3::ZERO;
            v[i] = if r & (1 << bit) != 0 { -1 } else { 1 };
            v
        };
        Some(Rot([row(i0, 4), row(i1, 5), row(i2, 6)]))
    }

    fn apply(&self, v: IVec3) -> IVec3 {
        IVec3::new(self.0[0].dot(v), self.0[1].dot(v), self.0[2].dot(v))
    }

    fn then(&self, inner: &Rot) -> Rot {
        // (self ∘ inner)(v) = self(inner(v)); rows of the product.
        let col = |j: usize| self.apply(IVec3::new(inner.0[0][j], inner.0[1][j], inner.0[2][j]));
        let (c0, c1, c2) = (col(0), col(1), col(2));
        Rot([
            IVec3::new(c0.x, c1.x, c2.x),
            IVec3::new(c0.y, c1.y, c2.y),
            IVec3::new(c0.z, c1.z, c2.z),
        ])
    }
}

fn parse_t(s: &str) -> IVec3 {
    let v: Vec<i32> = s
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    match v.as_slice() {
        [x, y, z] => IVec3::new(*x, *y, *z).clamp(IVec3::splat(-100_000), IVec3::splat(100_000)),
        _ => IVec3::ZERO,
    }
}

/// One placed model.
struct Instance {
    model: usize,
    rot: Rot,
    t: IVec3,
    hidden: bool,
    name: Option<String>,
    group: Option<LayerId>,
}

struct GroupOut {
    id: LayerId,
    name: String,
    hidden: bool,
    parent: Option<LayerId>,
    /// Instances found before the group: keeps groups and models in scene
    /// order.
    at: usize,
}

/// Parses a `.vox` file into a project named `name`.
pub fn read_vox(bytes: &[u8], name: &str) -> Result<ProjectFile, RzxlError> {
    let mut r = Reader {
        data: bytes,
        pos: 0,
    };
    if r.bytes(4)? != b"VOX " {
        return Err(invalid("not a MagicaVoxel file (bad magic)"));
    }
    let _version = r.i32()?;
    if r.bytes(4)? != b"MAIN" {
        return Err(invalid("missing MAIN chunk"));
    }
    let main_content = r.count(bytes.len(), "MAIN content")?;
    let _children = r.count(bytes.len(), "MAIN children")?;
    r.bytes(main_content)?;

    let mut models: Vec<Model> = Vec::new();
    let mut pending_size: Option<IVec3> = None;
    let mut nodes: HashMap<i32, Node> = HashMap::new();
    let mut hidden_layers: HashSet<i32> = HashSet::new();
    let mut rgba: Option<[Rgba8; 256]> = None;

    while r.pos + 12 <= bytes.len() {
        let id: [u8; 4] = r.bytes(4)?.try_into().map_err(|_| invalid("chunk id"))?;
        let content = r.count(bytes.len(), "chunk size")?;
        let children = r.count(bytes.len(), "chunk children")?;
        let body = r.bytes(content)?;
        // Children of non-MAIN chunks are not used by any known writer.
        r.bytes(children)?;
        let mut c = Reader { data: body, pos: 0 };
        match &id {
            b"SIZE" => {
                let s = IVec3::new(c.i32()?, c.i32()?, c.i32()?);
                if s.cmplt(IVec3::ONE).any() || s.cmpgt(IVec3::splat(2048)).any() {
                    return Err(invalid(format!("bad model size {s}")));
                }
                pending_size = Some(s);
            }
            b"XYZI" => {
                let size = pending_size
                    .take()
                    .ok_or_else(|| invalid("XYZI without SIZE"))?;
                if models.len() >= MAX_MODELS {
                    return Err(invalid("too many models"));
                }
                let n = c.count(body.len() / 4, "voxel")?;
                let voxels = (0..n)
                    .map(|_| c.bytes(4).map(|b| [b[0], b[1], b[2], b[3]]))
                    .collect::<Result<Vec<_>, _>>()?;
                models.push(Model { size, voxels });
            }
            b"nTRN" => {
                let node = c.i32()?;
                let attrs = c.dict()?;
                let child = c.i32()?;
                let _reserved = c.i32()?;
                let layer = c.i32()?;
                let frames = c.count(MAX_NODES, "frame")?;
                let mut frame = HashMap::new();
                for i in 0..frames {
                    let d = c.dict()?;
                    if i == 0 {
                        frame = d;
                    }
                }
                insert_node(
                    &mut nodes,
                    node,
                    Node::Transform {
                        attrs,
                        child,
                        layer,
                        frame,
                    },
                )?;
            }
            b"nGRP" => {
                let node = c.i32()?;
                let _attrs = c.dict()?;
                let n = c.count(MAX_NODES, "group child")?;
                let children = (0..n).map(|_| c.i32()).collect::<Result<Vec<_>, _>>()?;
                insert_node(&mut nodes, node, Node::Group { children })?;
            }
            b"nSHP" => {
                let node = c.i32()?;
                let _attrs = c.dict()?;
                let n = c.count(MAX_MODELS, "shape model")?;
                let mut list = Vec::new();
                for _ in 0..n {
                    list.push(c.i32()?);
                    let _model_attrs = c.dict()?;
                }
                insert_node(&mut nodes, node, Node::Shape { models: list })?;
            }
            b"LAYR" => {
                let layer = c.i32()?;
                let attrs = c.dict()?;
                if attrs.get("_hidden").is_some_and(|v| v == "1") {
                    hidden_layers.insert(layer);
                }
            }
            b"RGBA" => {
                let mut pal = [Rgba8::new(0, 0, 0, 0); 256];
                for slot in pal.iter_mut() {
                    let b = c.bytes(4)?;
                    *slot = Rgba8::new(b[0], b[1], b[2], b[3]);
                }
                rgba = Some(pal);
            }
            _ => {}
        }
    }
    if models.is_empty() {
        return Err(invalid("the file contains no models"));
    }

    // Scene graph -> placed instances and groups.
    let mut instances: Vec<Instance> = Vec::new();
    let mut groups: Vec<GroupOut> = Vec::new();
    let mut next_id: LayerId = 1;
    if nodes.contains_key(&0) {
        let mut walk = Walk {
            nodes: &nodes,
            hidden_layers: &hidden_layers,
            model_count: models.len(),
            instances: &mut instances,
            groups: &mut groups,
            next_id: &mut next_id,
            visiting: HashSet::new(),
        };
        walk.node(0, Rot::IDENTITY, IVec3::ZERO, false, None, None, 0, true)?;
    }
    if instances.is_empty() {
        // Version 150 files (or an empty scene): every model at the origin.
        for (i, m) in models.iter().enumerate() {
            instances.push(Instance {
                model: i,
                rot: Rot::IDENTITY,
                t: m.size / 2,
                hidden: false,
                name: None,
                group: None,
            });
        }
    }

    // World cell of a model voxel (MagicaVoxel integer pivot).
    let place = |inst: &Instance, p: IVec3| -> IVec3 {
        inst.rot.apply(p - models[inst.model].size / 2) + inst.t
    };
    // Scene extent from the full model boxes, so sizes survive a round trip.
    let (mut lo, mut hi) = (IVec3::splat(i32::MAX), IVec3::splat(i32::MIN));
    for inst in &instances {
        let s = models[inst.model].size - IVec3::ONE;
        for corner in 0..8 {
            let c = IVec3::new(
                if corner & 1 == 0 { 0 } else { s.x },
                if corner & 2 == 0 { 0 } else { s.y },
                if corner & 4 == 0 { 0 } else { s.z },
            );
            let w = place(inst, c);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    let zsize = hi - lo + IVec3::ONE;
    let rsize = UVec3::new(zsize.x as u32, zsize.z as u32, zsize.y as u32);
    let bounds = bounds_for(rsize, "The MagicaVoxel scene")?;

    // Palette: color index i -> Rizxel slot i.
    let src = rgba.unwrap_or_else(default_palette);
    let mut entries = vec![Rgba8::BLACK];
    entries.extend(src.iter().take(255).copied());
    let palette = Palette::new(entries);

    let group_layer = |g: &GroupOut| ImportLayer {
        data: LayerData {
            kind: LayerKind::Group { collapsed: false },
            visible: !g.hidden,
            parent: g.parent,
            ..LayerData::new(g.id, g.name.clone())
        },
        volume: None,
    };
    let mut pending_groups = groups.iter().peekable();
    let mut layers: Vec<ImportLayer> = Vec::new();
    // First instance per (model, rotation) owns the voxels; later ones link.
    let mut owners: HashMap<(usize, Rot), (LayerId, IVec3)> = HashMap::new();
    for (k, inst) in instances.iter().enumerate() {
        while let Some(g) = pending_groups.next_if(|g| g.at <= k) {
            layers.push(group_layer(g));
        }
        let id = next_id;
        next_id += 1;
        let name = inst
            .name
            .clone()
            .unwrap_or_else(|| format!("Model {}", k + 1));
        let mut data = LayerData {
            visible: !inst.hidden,
            parent: inst.group,
            ..LayerData::new(id, name)
        };
        let volume = match owners.get(&(inst.model, inst.rot)) {
            Some(&(source, t0)) => {
                data.kind = LayerKind::Clone {
                    source,
                    offset: dir_from_zup(inst.t - t0),
                };
                None
            }
            None => {
                owners.insert((inst.model, inst.rot), (id, inst.t));
                let mut v = VoxelVolume::new(bounds);
                for &[x, y, z, c] in &models[inst.model].voxels {
                    if c != 0 {
                        let w = place(inst, IVec3::new(x as i32, y as i32, z as i32));
                        v.set(from_zup(zsize, w - lo), c);
                    }
                }
                Some(v)
            }
        };
        layers.push(ImportLayer { data, volume });
    }
    layers.extend(pending_groups.map(group_layer));
    build_project(name, bounds, palette, Vec::new(), layers)
}

fn insert_node(nodes: &mut HashMap<i32, Node>, id: i32, node: Node) -> Result<(), RzxlError> {
    if nodes.len() >= MAX_NODES {
        return Err(invalid("too many scene nodes"));
    }
    nodes.insert(id, node);
    Ok(())
}

struct Walk<'a> {
    nodes: &'a HashMap<i32, Node>,
    hidden_layers: &'a HashSet<i32>,
    model_count: usize,
    instances: &'a mut Vec<Instance>,
    groups: &'a mut Vec<GroupOut>,
    next_id: &'a mut LayerId,
    visiting: HashSet<i32>,
}

impl Walk<'_> {
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        id: i32,
        rot: Rot,
        t: IVec3,
        hidden: bool,
        name: Option<String>,
        group: Option<LayerId>,
        depth: usize,
        root: bool,
    ) -> Result<(), RzxlError> {
        if depth > MAX_SCENE_DEPTH || !self.visiting.insert(id) {
            return Err(invalid("scene graph is cyclic or too deep"));
        }
        match self.nodes.get(&id) {
            Some(Node::Transform {
                attrs,
                child,
                layer,
                frame,
            }) => {
                let r = frame
                    .get("_r")
                    .and_then(|s| Rot::parse(s))
                    .unwrap_or(Rot::IDENTITY);
                let local_t = frame.get("_t").map(|s| parse_t(s)).unwrap_or(IVec3::ZERO);
                let hidden = hidden
                    || attrs.get("_hidden").is_some_and(|v| v == "1")
                    || self.hidden_layers.contains(layer);
                let name = attrs.get("_name").cloned().or(name);
                self.node(
                    *child,
                    rot.then(&r),
                    rot.apply(local_t) + t,
                    hidden,
                    name,
                    group,
                    depth + 1,
                    root,
                )?;
            }
            Some(Node::Group { children }) => {
                // The top-level group is the scene itself; nested ones
                // become Rizxel groups.
                let group = if root {
                    group
                } else {
                    let gid = *self.next_id;
                    *self.next_id += 1;
                    self.groups.push(GroupOut {
                        id: gid,
                        name: name.unwrap_or_else(|| format!("Group {gid}")),
                        hidden,
                        parent: group,
                        at: self.instances.len(),
                    });
                    Some(gid)
                };
                for &c in children {
                    self.node(c, rot, t, hidden, None, group, depth + 1, false)?;
                }
            }
            Some(Node::Shape { models }) => {
                for &m in models {
                    let model = usize::try_from(m)
                        .ok()
                        .filter(|&m| m < self.model_count)
                        .ok_or_else(|| invalid(format!("shape uses missing model {m}")))?;
                    self.instances.push(Instance {
                        model,
                        rot,
                        t,
                        hidden,
                        name: name.clone(),
                        group,
                    });
                }
            }
            None => return Err(invalid(format!("scene uses missing node {id}"))),
        }
        self.visiting.remove(&id);
        Ok(())
    }
}

// ------------------------------------------------------------------ writing

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn string(&mut self, s: &str) {
        self.i32(s.len() as i32);
        self.out.extend_from_slice(s.as_bytes());
    }

    fn dict(&mut self, entries: &[(&str, String)]) {
        self.i32(entries.len() as i32);
        for (k, v) in entries {
            self.string(k);
            self.string(v);
        }
    }

    /// Appends a chunk with no children; `body` writes its content.
    fn chunk(&mut self, id: &[u8; 4], body: impl FnOnce(&mut Writer)) {
        let mut inner = Writer { out: Vec::new() };
        body(&mut inner);
        self.out.extend_from_slice(id);
        self.i32(inner.out.len() as i32);
        self.i32(0);
        self.out.extend_from_slice(&inner.out);
    }
}

/// A non-empty layer written as model `k` (its position in the kept
/// list); `index` is its position in [`ExportScene::layers`].
struct KeptLayer<'a> {
    index: usize,
    name: &'a str,
    visible: bool,
    volume: &'a VoxelVolume,
    group: Option<usize>,
}

/// A child node of a scene group.
#[derive(Clone, Copy)]
enum SceneItem {
    Model(usize),
    Group(usize),
}

/// Writes the transform / shape / group chunks below the scene root.
struct SceneWriter<'a> {
    w: &'a mut Writer,
    scene: &'a ExportScene,
    layers: &'a [KeptLayer<'a>],
    items: &'a HashMap<Option<usize>, Vec<(usize, SceneItem)>>,
    next_id: i32,
    /// `_t` of every model transform.
    translation: String,
    /// Groups already written (guards against cyclic `parent` links).
    written: HashSet<usize>,
}

impl SceneWriter<'_> {
    /// Writes the children of `parent` and returns their transform ids.
    fn children(&mut self, parent: Option<usize>, depth: usize) -> Result<Vec<i32>, RzxlError> {
        if depth > MAX_SCENE_DEPTH / 2 {
            return Err(invalid("groups are nested too deep for .vox"));
        }
        let mut ids = Vec::new();
        for &(_, item) in self.items.get(&parent).map_or(&[][..], Vec::as_slice) {
            let id = self.next_id;
            self.next_id += 2;
            ids.push(id);
            let (name, visible) = match item {
                SceneItem::Model(k) => (self.layers[k].name, self.layers[k].visible),
                SceneItem::Group(g) => {
                    let group = &self.scene.groups[g];
                    (group.name.as_str(), group.visible)
                }
            };
            let mut attrs = vec![("_name", name.to_owned())];
            if !visible {
                attrs.push(("_hidden", "1".to_owned()));
            }
            let frame: Vec<(&str, String)> = match item {
                SceneItem::Model(_) => vec![("_t", self.translation.clone())],
                SceneItem::Group(_) => Vec::new(),
            };
            self.w.chunk(b"nTRN", |c| {
                c.i32(id);
                c.dict(&attrs);
                c.i32(id + 1);
                c.i32(-1);
                c.i32(0);
                c.i32(1);
                c.dict(&frame);
            });
            match item {
                SceneItem::Model(k) => self.w.chunk(b"nSHP", |c| {
                    c.i32(id + 1);
                    c.dict(&[]);
                    c.i32(1);
                    c.i32(k as i32);
                    c.dict(&[]);
                }),
                SceneItem::Group(g) => {
                    if !self.written.insert(g) {
                        return Err(invalid("cyclic groups"));
                    }
                    let kids = self.children(Some(g), depth + 1)?;
                    self.w.chunk(b"nGRP", |c| {
                        c.i32(id + 1);
                        c.dict(&[]);
                        c.i32(kids.len() as i32);
                        for kid in &kids {
                            c.i32(*kid);
                        }
                    });
                }
            }
        }
        Ok(ids)
    }
}

/// Encodes a scene as a `.vox` file. Fails when no layer has voxels.
pub fn write_vox(scene: &ExportScene) -> Result<Vec<u8>, RzxlError> {
    let layers: Vec<KeptLayer> = scene
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.volume.is_empty())
        .map(|(index, l)| KeptLayer {
            index,
            name: &l.name,
            visible: l.visible,
            volume: &l.volume,
            group: l.group,
        })
        .collect();
    if layers.is_empty() {
        return Err(invalid("nothing to export: no layer contains voxels"));
    }
    let size = zup_size(scene.bounds);
    let mut w = Writer { out: Vec::new() };
    for l in &layers {
        w.chunk(b"SIZE", |c| {
            c.i32(size.x);
            c.i32(size.y);
            c.i32(size.z);
        });
        let mut cells: Vec<(IVec3, u8)> = l
            .volume
            .iter()
            .map(|(p, v)| (to_zup(scene.bounds, p), v))
            .collect();
        cells.sort_by_key(|(p, _)| (p.z, p.y, p.x));
        w.chunk(b"XYZI", |c| {
            c.i32(cells.len() as i32);
            for (p, v) in &cells {
                c.out
                    .extend_from_slice(&[p.x as u8, p.y as u8, p.z as u8, *v]);
            }
        });
    }
    // Scene: root transform -> group -> (transform -> shape) per model,
    // and (transform -> group) per Rizxel group. Each model translation
    // equals the integer half size, so model space == world.
    let half = size / 2;
    let t = format!("{} {} {}", half.x, half.y, half.z);
    w.chunk(b"nTRN", |c| {
        c.i32(0);
        c.dict(&[]);
        c.i32(1);
        c.i32(-1);
        c.i32(-1);
        c.i32(1);
        c.dict(&[]);
    });
    // Children of the scene root (`None`) and of each group, in draw order.
    let mut items: HashMap<Option<usize>, Vec<(usize, SceneItem)>> = HashMap::new();
    for (k, l) in layers.iter().enumerate() {
        let parent = l.group.filter(|&g| g < scene.groups.len());
        items
            .entry(parent)
            .or_default()
            .push((2 * l.index + 1, SceneItem::Model(k)));
    }
    for (g, group) in scene.groups.iter().enumerate() {
        let parent = group.parent.filter(|&p| p < scene.groups.len() && p != g);
        items
            .entry(parent)
            .or_default()
            .push((2 * group.order, SceneItem::Group(g)));
    }
    for list in items.values_mut() {
        list.sort_by_key(|&(key, _)| key);
    }
    let mut scene_writer = SceneWriter {
        w: &mut w,
        scene,
        layers: &layers,
        items: &items,
        next_id: 2,
        translation: t,
        written: HashSet::new(),
    };
    let top = scene_writer.children(None, 0)?;
    w.chunk(b"nGRP", |c| {
        c.i32(1);
        c.dict(&[]);
        c.i32(top.len() as i32);
        for id in &top {
            c.i32(*id);
        }
    });
    w.chunk(b"LAYR", |c| {
        c.i32(0);
        c.dict(&[]);
        c.i32(-1);
    });
    w.chunk(b"RGBA", |c| {
        for color in export_palette(&scene.palette) {
            c.out
                .extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
    });

    let mut out = Vec::with_capacity(w.out.len() + 20);
    out.extend_from_slice(b"VOX ");
    out.extend_from_slice(&VOX_VERSION.to_le_bytes());
    out.extend_from_slice(b"MAIN");
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&(w.out.len() as i32).to_le_bytes());
    out.extend_from_slice(&w.out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::{ExportGroup, ExportLayer};
    use crate::volume_from_runs;
    use rizxel_math::BoundSize;

    fn scene() -> ExportScene {
        let bounds = BoundSize::new(5, 7, 3);
        let mut a = VoxelVolume::new(bounds);
        a.set(IVec3::new(0, 0, 0), 1);
        a.set(IVec3::new(4, 6, 2), 200);
        a.set(IVec3::new(1, 2, 0), 9);
        let mut b = VoxelVolume::new(bounds);
        b.set(IVec3::new(2, 3, 1), 42);
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers: vec![
                ExportLayer::new("Body", true, a),
                ExportLayer::new("Hat", false, b),
                ExportLayer::new("Empty", true, VoxelVolume::new(bounds)),
            ],
            groups: Vec::new(),
        }
    }

    #[test]
    fn default_palette_matches_magicavoxel() {
        let p = default_palette();
        assert_eq!(p[0], Rgba8::WHITE);
        assert_eq!(p[1], Rgba8::rgb(0xff, 0xff, 0xcc));
        assert_eq!(p[214], Rgba8::rgb(0, 0, 0x33));
        assert_eq!(p[215], Rgba8::rgb(0xee, 0, 0));
        assert_eq!(p[254], Rgba8::rgb(0x11, 0x11, 0x11));
        assert_eq!(p[255], Rgba8::new(0, 0, 0, 0));
    }

    #[test]
    fn export_import_roundtrip() {
        let s = scene();
        let bytes = write_vox(&s).unwrap();
        assert_eq!(&bytes[0..4], b"VOX ");
        let p = read_vox(&bytes, "t").unwrap();
        assert_eq!(p.bounds, s.bounds);
        assert_eq!(p.layers.len(), 2, "empty layers are skipped");
        assert_eq!(p.layers[0].name, "Body");
        assert!(p.layers[0].visible && !p.layers[1].visible);
        for (data, src) in p.layers.iter().zip(&s.layers) {
            let v = volume_from_runs(p.bounds, &data.voxels).unwrap();
            assert_eq!(v, src.volume);
        }
        // Palette indices map one to one.
        for i in 1..s.palette.len() {
            assert_eq!(p.palette.get(i), s.palette.get(i));
        }
    }

    #[test]
    fn groups_are_written_as_nested_group_nodes() {
        // Body, then [Hat, [Empty] Inner] Outer: the empty layer is skipped
        // but its group stays.
        let mut s = scene();
        s.layers[1].group = Some(1);
        s.layers[2].group = Some(0);
        s.groups = vec![
            ExportGroup {
                name: "Inner".into(),
                visible: true,
                parent: Some(1),
                order: 3,
            },
            ExportGroup {
                name: "Outer".into(),
                visible: false,
                parent: None,
                order: 3,
            },
        ];
        let p = read_vox(&write_vox(&s).unwrap(), "t").unwrap();
        let by_name = |n: &str| p.layers.iter().find(|l| l.name == n).unwrap();
        let (outer, inner, hat) = (by_name("Outer"), by_name("Inner"), by_name("Hat"));
        assert!(outer.kind.is_group() && inner.kind.is_group());
        assert_eq!(outer.parent, None);
        assert_eq!(inner.parent, Some(outer.id));
        assert_eq!(hat.parent, Some(outer.id));
        assert_eq!(by_name("Body").parent, None);
        assert!(
            !outer.visible && !hat.visible,
            "hidden groups hide their contents"
        );
        assert_eq!(p.layers.iter().filter(|l| !l.kind.is_group()).count(), 2);
        // Cyclic group links are refused instead of looping.
        s.groups[1].parent = Some(0);
        assert!(
            write_vox(&s).is_ok(),
            "both groups unreachable from the root"
        );
    }

    #[test]
    fn rotations_compose_and_parse() {
        // _r = 4: row0 -> x, row1 -> y ... MagicaVoxel identity is 4.
        assert_eq!(Rot::parse("4"), Some(Rot::IDENTITY));
        // 90° about Z: x' = -y, y' = x  =>  rows (0,-1,0), (1,0,0), (0,0,1).
        let r = Rot([IVec3::NEG_Y, IVec3::X, IVec3::Z]);
        // Row 0 on column 1 (bits 0-1 = 1), row 1 on column 0 (bits 2-3 =
        // 0), row 0 negative (bit 4): 1 + 16 = 17.
        assert_eq!(Rot::parse("17"), Some(r));
        assert_eq!(r.apply(IVec3::X), IVec3::Y);
        assert_eq!(r.then(&r).apply(IVec3::X), IVec3::NEG_X);
        assert_eq!(Rot::parse("0"), None, "two rows on the same axis");
    }

    #[test]
    fn repeated_instances_become_linked_clones() {
        // Hand-built scene: one model shown twice, 10 voxels apart in X,
        // inside a named, nested group.
        let mut w = Writer { out: Vec::new() };
        w.chunk(b"SIZE", |c| {
            c.i32(2);
            c.i32(2);
            c.i32(2);
        });
        w.chunk(b"XYZI", |c| {
            c.i32(1);
            c.out.extend_from_slice(&[0, 0, 0, 7]);
        });
        let trn = |w: &mut Writer, id: i32, child: i32, attrs: &[(&str, String)], t: &str| {
            w.chunk(b"nTRN", |c| {
                c.i32(id);
                c.dict(attrs);
                c.i32(child);
                c.i32(-1);
                c.i32(0);
                c.i32(1);
                c.dict(&[("_t", t.to_owned())]);
            });
        };
        trn(&mut w, 0, 1, &[], "0 0 0");
        w.chunk(b"nGRP", |c| {
            c.i32(1);
            c.dict(&[]);
            c.i32(1);
            c.i32(2);
        });
        trn(&mut w, 2, 3, &[("_name", "Pair".into())], "0 0 0");
        w.chunk(b"nGRP", |c| {
            c.i32(3);
            c.dict(&[]);
            c.i32(2);
            c.i32(4);
            c.i32(6);
        });
        for (id, t) in [(4, "0 0 0"), (6, "10 0 0")] {
            trn(&mut w, id, id + 1, &[], t);
            w.chunk(b"nSHP", |c| {
                c.i32(id + 1);
                c.dict(&[]);
                c.i32(1);
                c.i32(0);
                c.dict(&[]);
            });
        }
        let mut file = b"VOX ".to_vec();
        file.extend_from_slice(&150i32.to_le_bytes());
        file.extend_from_slice(b"MAIN");
        file.extend_from_slice(&0i32.to_le_bytes());
        file.extend_from_slice(&(w.out.len() as i32).to_le_bytes());
        file.extend_from_slice(&w.out);

        let p = read_vox(&file, "pair").unwrap();
        assert_eq!(p.bounds, BoundSize::new(12, 2, 2));
        assert_eq!(p.layers.len(), 3);
        assert_eq!(p.layers[0].name, "Pair");
        assert!(p.layers[0].kind.is_group());
        assert_eq!(p.layers[1].parent, Some(p.layers[0].id));
        assert_eq!(
            p.layers[2].kind,
            LayerKind::Clone {
                source: p.layers[1].id,
                offset: IVec3::new(10, 0, 0)
            }
        );
    }

    #[test]
    fn hostile_files_fail_cleanly() {
        assert!(read_vox(b"", "x").is_err());
        assert!(read_vox(b"VOX \x96\0\0\0MAIN\xff\xff\xff\x7f\0\0\0\0", "x").is_err());
        let mut bytes = write_vox(&scene()).unwrap();
        // Truncate in the middle of a chunk.
        bytes.truncate(bytes.len() / 2);
        let _ = read_vox(&bytes, "x"); // must not panic
                                       // Claim far more voxels than the chunk holds.
        let mut bytes = write_vox(&scene()).unwrap();
        let at = bytes.windows(4).position(|w| w == b"XYZI").unwrap() + 12;
        bytes[at..at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(read_vox(&bytes, "x").is_err());
    }

    #[test]
    fn reads_magicavoxel_sample_when_installed() {
        let Some(path) = crate::test_samples::sample("chr_knight.vox") else {
            return; // sample not available on this machine
        };
        let Ok(bytes) = std::fs::read(path) else {
            return; // sample not available on this machine
        };
        let p = read_vox(&bytes, "chr_knight").unwrap();
        // 20×21×20 (Z-up) -> Rizxel 20 wide, 20 tall, 21 deep.
        assert_eq!(p.bounds, BoundSize::new(20, 20, 21));
        let count: u32 = p.layers.iter().flat_map(|l| &l.voxels).map(|r| r.len).sum();
        assert_eq!(count, 398);
    }
}
