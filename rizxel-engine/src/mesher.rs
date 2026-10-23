//! Greedy voxel mesher and the indexed triangle mesh it produces.
//!
//! [`greedy_mesh`] turns the visible layers of a document into one mesh in
//! model space (voxel `v` occupies `[v, v + 1)`). Each 16³ tile is swept along
//! the three axes in both directions; faces whose neighbour across the face is
//! empty (or outside the bounds, so the shell is always closed) are collected
//! into a 16×16 mask per slice and merged greedily into rectangles of one
//! palette index and material slot. Quads never merge across tile borders.
//!
//! With [`MeshParams::ao`] every face corner also gets a classic 4-level
//! ambient occlusion value from the three voxels around it in front of the
//! face; only cells with the same four values merge, and each quad's
//! diagonal is chosen so the occlusion interpolates without anisotropy.
//!
//! Because a tile's quads depend only on that tile and the boundary voxels
//! of its neighbours (face neighbours, plus edge and corner neighbours for
//! AO), [`TileMeshCache`] can re-mesh just the tiles a drag changed and
//! concatenate the rest from its cache; the result is identical to a full
//! [`mesh_volume_with`].

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{IVec3, Vec3};
use rizxel_math::volume::{local_coord, local_index};
use rizxel_math::{Palette, Rgba8, Tile, Voxel, VoxelVolume, EMPTY_VOXEL, TILE_SIZE};

/// Indexed triangle mesh. All per-vertex arrays have the same length.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<Rgba8>,
    /// Material slot per vertex (see [`MaterialSlots`]); may be empty.
    pub materials: Vec<u8>,
    /// Ambient occlusion per vertex: 0 (most occluded) to [`AO_OPEN`]
    /// (open); may be empty.
    pub ao: Vec<u8>,
    /// Triangle list; every 3 indices form one triangle.
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Checks structural invariants (equal attribute lengths, valid indices).
    pub fn is_valid(&self) -> bool {
        let n = self.positions.len();
        (self.normals.is_empty() || self.normals.len() == n)
            && (self.colors.is_empty() || self.colors.len() == n)
            && (self.materials.is_empty() || self.materials.len() == n)
            && (self.ao.is_empty() || self.ao.len() == n)
            && self.indices.len() % 3 == 0
            && self.indices.iter().all(|&i| (i as usize) < n)
    }

    /// Appends `other`, shifting its indices.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.materials.extend_from_slice(&other.materials);
        self.ao.extend_from_slice(&other.ao);
        self.indices.extend(other.indices.iter().map(|i| base + i));
    }

    /// Unit cube with 8 shared vertices and 12 triangles (no normals).
    /// Useful for tests and as a placeholder mesh.
    pub fn unit_cube(color: Rgba8) -> Self {
        let positions = (0..8u32)
            .map(|i| Vec3::new((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32))
            .collect::<Vec<_>>();

        // Counter-clockwise winding viewed from outside.
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 3, 0, 3, 1, // -Z
            4, 5, 7, 4, 7, 6, // +Z
            0, 4, 6, 0, 6, 2, // -X
            1, 3, 7, 1, 7, 5, // +X
            0, 1, 5, 0, 5, 4, // -Y
            2, 6, 7, 2, 7, 3, // +Y
        ];

        Self {
            colors: vec![color; positions.len()],
            positions,
            normals: Vec::new(),
            materials: Vec::new(),
            ao: Vec::new(),
            indices,
        }
    }
}

/// Material slot of each voxel, copied to its vertices. The renderer looks
/// slots up in its material table: 0 is the default material, `n` the
/// document's material `n - 1`. Quads never merge across slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialSlots<'a> {
    /// Every voxel uses this slot.
    Uniform(u8),
    /// Slot per cell (empty cells read as slot 0).
    PerVoxel(&'a VoxelVolume),
}

impl MaterialSlots<'_> {
    fn get(&self, pos: IVec3) -> u8 {
        match self {
            MaterialSlots::Uniform(slot) => *slot,
            MaterialSlots::PerVoxel(volume) => volume.get(pos),
        }
    }
}

/// Owned copy of [`MaterialSlots`], kept by [`TileMeshCache`].
#[derive(Debug, Clone, PartialEq)]
enum SlotSource {
    Uniform(u8),
    PerVoxel(VoxelVolume),
}

impl SlotSource {
    fn new(slots: MaterialSlots<'_>) -> Self {
        match slots {
            MaterialSlots::Uniform(slot) => SlotSource::Uniform(slot),
            MaterialSlots::PerVoxel(v) => SlotSource::PerVoxel(v.clone()),
        }
    }

    /// Same kind (and the same slot when uniform): only per-voxel tiles can
    /// differ.
    fn compatible(&self, slots: MaterialSlots<'_>) -> bool {
        match (self, slots) {
            (SlotSource::Uniform(a), MaterialSlots::Uniform(b)) => *a == b,
            (SlotSource::PerVoxel(a), MaterialSlots::PerVoxel(b)) => a.bounds() == b.bounds(),
            _ => false,
        }
    }

    fn tile_differs(&self, slots: MaterialSlots<'_>, origin: IVec3) -> bool {
        match (self, slots) {
            (SlotSource::PerVoxel(a), MaterialSlots::PerVoxel(b)) => {
                a.tile(origin) != b.tile(origin)
            }
            _ => false,
        }
    }
}

/// AO value of an unoccluded vertex.
pub const AO_OPEN: u8 = 3;

/// Everything besides the voxels that decides a mesh.
#[derive(Debug, Clone, Copy)]
pub struct MeshParams<'a> {
    /// Vertex colors.
    pub palette: &'a Palette,
    pub slots: MaterialSlots<'a>,
    /// Per-vertex ambient occlusion (viewport). Off, every vertex is
    /// [`AO_OPEN`] and quads merge as large as possible (exports).
    pub ao: bool,
}

impl<'a> MeshParams<'a> {
    /// Plain mesh: palette colors, every voxel in material slot 0, no AO.
    pub fn new(palette: &'a Palette) -> Self {
        Self {
            palette,
            slots: MaterialSlots::Uniform(0),
            ao: false,
        }
    }

    pub fn slots(self, slots: MaterialSlots<'a>) -> Self {
        Self { slots, ..self }
    }

    pub fn ao(self, ao: bool) -> Self {
        Self { ao, ..self }
    }
}

/// Meshes the visible layers into one mesh. Layers later in the slice win
/// where two visible layers overlap. Vertex colors come from `palette`.
pub fn greedy_mesh(layers: &[(&VoxelVolume, bool)], palette: &Palette) -> Mesh {
    let visible: Vec<&VoxelVolume> = layers
        .iter()
        .filter(|(v, visible)| *visible && !v.is_empty())
        .map(|(v, _)| *v)
        .collect();
    match visible.as_slice() {
        [] => Mesh::default(),
        [single] => mesh_volume(single, palette),
        [first, rest @ ..] => {
            let mut merged = (*first).clone();
            for layer in rest {
                for (pos, v) in layer.iter() {
                    merged.set(pos, v);
                }
            }
            mesh_volume(&merged, palette)
        }
    }
}

/// Greedy-meshes a single volume with [`MeshParams::new`].
pub fn mesh_volume(volume: &VoxelVolume, palette: &Palette) -> Mesh {
    mesh_volume_with(volume, &MeshParams::new(palette))
}

/// Greedy-meshes a single volume (see the module docs).
pub fn mesh_volume_with(volume: &VoxelVolume, params: &MeshParams<'_>) -> Mesh {
    let mut mesher = Mesher::new(params);
    // Sorted tiles keep the output byte-identical across runs.
    for origin in volume.sorted_tile_origins() {
        mesher.tile(volume, origin);
    }
    mesher.mesh
}

/// Quads of the single tile at `origin` (in model space).
pub fn mesh_tile(volume: &VoxelVolume, origin: IVec3, params: &MeshParams<'_>) -> Mesh {
    let mut mesher = Mesher::new(params);
    mesher.tile(volume, origin);
    mesher.mesh
}

/// Per-tile meshes of the last volume meshed, for incremental re-meshing
/// while a drag edits a few tiles per frame.
///
/// [`TileMeshCache::mesh_shared`] compares each tile with the previous
/// volume and re-meshes only the changed ones and their neighbours (border
/// faces may have appeared or vanished; with AO, corner values read the
/// edge and corner neighbours too). A palette, bounds, slot-kind or AO
/// change, or the first call, rebuilds everything. The output
/// always equals [`mesh_volume_with`] of the same volume.
#[derive(Debug, Default)]
pub struct TileMeshCache {
    source: Option<VoxelVolume>,
    palette: Option<Palette>,
    slots: Option<SlotSource>,
    ao: bool,
    tiles: HashMap<IVec3, Mesh>,
    /// Tiles re-meshed by the last call (diagnostics and tests).
    last_rebuilt: usize,
    /// Concatenated output of the last call, shared while nothing changes.
    output: Option<Arc<Mesh>>,
}

impl TileMeshCache {
    /// Mesh of `volume` with [`MeshParams::new`], reusing the cached tiles
    /// that did not change.
    pub fn mesh(&mut self, volume: &VoxelVolume, palette: &Palette) -> Mesh {
        (*self.mesh_shared(volume, &MeshParams::new(palette))).clone()
    }

    /// Mesh of `volume`; returns the previous `Arc` when no tile changed, so
    /// callers can skip uploading an identical mesh.
    pub fn mesh_shared(&mut self, volume: &VoxelVolume, params: &MeshParams<'_>) -> Arc<Mesh> {
        let before = self.tiles.len();
        self.update(volume, params);
        let changed = self.last_rebuilt > 0 || self.tiles.len() != before;
        match &self.output {
            Some(out) if !changed => out.clone(),
            _ => {
                let out = Arc::new(self.concatenate(volume));
                self.output = Some(out.clone());
                out
            }
        }
    }

    /// Re-meshes the tiles that changed since the last call.
    fn update(&mut self, volume: &VoxelVolume, params: &MeshParams<'_>) {
        let full = self.palette.as_ref() != Some(params.palette)
            || self.ao != params.ao
            || self
                .slots
                .as_ref()
                .is_none_or(|s| !s.compatible(params.slots))
            || self
                .source
                .as_ref()
                .is_none_or(|s| s.bounds() != volume.bounds());
        let origins = volume.sorted_tile_origins();
        let dirty: HashSet<IVec3> = match (&self.source, &self.slots, full) {
            (Some(old), Some(old_slots), false) => {
                let mut candidates: HashSet<IVec3> = origins.iter().copied().collect();
                candidates.extend(old.sorted_tile_origins());
                let near = neighbour_offsets(params.ao);
                let mut dirty = HashSet::new();
                for o in candidates {
                    let (before, after) = (old.tile(o), volume.tile(o));
                    if before != after {
                        // Neighbours read one voxel into this tile: only
                        // those whose side the changes touch need a re-mesh.
                        let (lo, hi) = diff_box(before, after);
                        dirty.extend(
                            near.iter()
                                .filter(|&&d| touches_side(d, lo, hi))
                                .map(|&d| o + d * TILE_SIZE),
                        );
                    } else if old_slots.tile_differs(params.slots, o) {
                        // Slots only affect the tile's own quads.
                        dirty.insert(o);
                    }
                }
                dirty
            }
            _ => {
                self.tiles.clear();
                origins.iter().copied().collect()
            }
        };
        self.tiles.retain(|o, _| !dirty.contains(o));
        let mut rebuilt = 0;
        for &o in &origins {
            if let Entry::Vacant(slot) = self.tiles.entry(o) {
                slot.insert(mesh_tile(volume, o, params));
                rebuilt += 1;
            }
        }
        self.last_rebuilt = rebuilt;
        self.source = Some(volume.clone());
        self.palette = Some(params.palette.clone());
        self.slots = Some(SlotSource::new(params.slots));
        self.ao = params.ao;
    }

    /// The cached tiles of `volume` in sorted order, as one mesh.
    fn concatenate(&self, volume: &VoxelVolume) -> Mesh {
        let mut out = Mesh::default();
        for o in &volume.sorted_tile_origins() {
            if let Some(m) = self.tiles.get(o) {
                out.append(m);
            }
        }
        out
    }

    /// Number of tiles the last call re-meshed.
    pub fn last_rebuilt(&self) -> usize {
        self.last_rebuilt
    }
}

const N: usize = TILE_SIZE as usize;

/// Directions (in tiles) of the tiles whose mesh can depend on a tile:
/// itself and its 6 face neighbours, or all 26 neighbours with AO (corner
/// values read diagonally).
fn neighbour_offsets(ao: bool) -> Vec<IVec3> {
    let mut out = Vec::new();
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                if ao || i32::abs(x) + i32::abs(y) + i32::abs(z) <= 1 {
                    out.push(IVec3::new(x, y, z));
                }
            }
        }
    }
    out
}

/// Box (local coordinates, inclusive) around the cells where two tiles
/// differ. Only called for tiles that differ.
fn diff_box(a: &Tile, b: &Tile) -> (IVec3, IVec3) {
    let mut lo = IVec3::splat(TILE_SIZE);
    let mut hi = IVec3::splat(-1);
    for i in 0..(N * N * N) {
        if a.get(i) != b.get(i) {
            let l = local_coord(i);
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    (lo, hi)
}

/// True when changes inside `lo..=hi` reach the side of the tile facing the
/// neighbour in direction `d` (meshes read one voxel across tile borders).
fn touches_side(d: IVec3, lo: IVec3, hi: IVec3) -> bool {
    (0..3).all(|k| match d[k] {
        -1 => lo[k] == 0,
        1 => hi[k] == TILE_SIZE - 1,
        _ => true,
    })
}

/// A face direction: `±axis`.
#[derive(Clone, Copy)]
struct Face {
    axis: usize,
    positive: bool,
}

impl Face {
    fn u(self) -> usize {
        (self.axis + 1) % 3
    }

    fn v(self) -> usize {
        (self.axis + 2) % 3
    }

    fn sign(self) -> i32 {
        if self.positive {
            1
        } else {
            -1
        }
    }
}

/// One mask cell: palette index in the low byte, material slot in the
/// next, then the four corner AO values (2 bits each, corners in `(u, v)`
/// order 00, 10, 11, 01); 0 = no face. Only equal keys merge.
type FaceKey = u32;

fn face_key(value: Voxel, slot: u8, ao: [u8; 4]) -> FaceKey {
    let ao = ao
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, &a)| acc | (a as u32) << (2 * i));
    value as u32 | (slot as u32) << 8 | ao << 16
}

/// Classic voxel AO of one face corner from the two edge neighbours and the
/// diagonal one in front of the face: both sides solid is fully occluded.
fn corner_ao(side_a: bool, side_b: bool, corner: bool) -> u8 {
    if side_a && side_b {
        0
    } else {
        AO_OPEN - side_a as u8 - side_b as u8 - corner as u8
    }
}

struct Mesher<'p> {
    mesh: Mesh,
    params: MeshParams<'p>,
}

impl<'p> Mesher<'p> {
    fn new(params: &MeshParams<'p>) -> Self {
        Self {
            mesh: Mesh::default(),
            params: *params,
        }
    }

    /// Emits every face of the tile at `origin`.
    fn tile(&mut self, volume: &VoxelVolume, origin: IVec3) {
        let tile = volume.tile(origin);
        for axis in 0..3 {
            for positive in [false, true] {
                self.sweep(volume, origin, tile, Face { axis, positive });
            }
        }
    }

    /// Emits the faces of one tile that point along `face`.
    fn sweep(&mut self, volume: &VoxelVolume, origin: IVec3, tile: &Tile, face: Face) {
        let (axis, u_axis, v_axis) = (face.axis, face.u(), face.v());
        let mut normal = IVec3::ZERO;
        normal[axis] = face.sign();
        let mut mask: [FaceKey; N * N] = [0; N * N];

        for slice in 0..N {
            let mut any = false;
            for v in 0..N {
                for u in 0..N {
                    let mut l = IVec3::ZERO;
                    l[axis] = slice as i32;
                    l[u_axis] = u as i32;
                    l[v_axis] = v as i32;
                    let value = tile.get(local_index(l));
                    let visible = value != EMPTY_VOXEL && {
                        let n = l + normal;
                        let neighbour = if (0..TILE_SIZE).contains(&n[axis]) {
                            tile.get(local_index(n))
                        } else {
                            // Out-of-bounds reads are empty: the shell stays closed.
                            volume.get(origin + n)
                        };
                        neighbour == EMPTY_VOXEL
                    };
                    mask[v * N + u] = if visible {
                        let p = origin + l;
                        let ao = if self.params.ao {
                            face_ao(volume, tile, origin, p + normal, u_axis, v_axis)
                        } else {
                            [AO_OPEN; 4]
                        };
                        face_key(value, self.params.slots.get(p), ao)
                    } else {
                        0
                    };
                    any |= visible;
                }
            }
            if any {
                self.merge_mask(&mut mask, origin, face, slice);
            }
        }
    }

    /// Greedily covers the mask with rectangles of equal key and emits them.
    fn merge_mask(&mut self, mask: &mut [FaceKey; N * N], origin: IVec3, face: Face, slice: usize) {
        for v in 0..N {
            let mut u = 0;
            while u < N {
                let key = mask[v * N + u];
                if key == 0 {
                    u += 1;
                    continue;
                }
                let mut w = 1;
                while u + w < N && mask[v * N + u + w] == key {
                    w += 1;
                }
                let mut h = 1;
                while v + h < N
                    && mask[(v + h) * N + u..(v + h) * N + u + w]
                        .iter()
                        .all(|&m| m == key)
                {
                    h += 1;
                }
                for row in v..v + h {
                    mask[row * N + u..row * N + u + w].fill(0);
                }

                let mut base = origin.as_vec3();
                base[face.axis] += slice as f32 + if face.positive { 1.0 } else { 0.0 };
                base[face.u()] += u as f32;
                base[face.v()] += v as f32;
                let mut du = Vec3::ZERO;
                du[face.u()] = w as f32;
                let mut dv = Vec3::ZERO;
                dv[face.v()] = h as f32;
                self.quad(base, du, dv, face, key);
                u += w;
            }
        }
    }

    /// Appends one quad (4 vertices, 2 triangles), counter-clockwise when seen
    /// from the side its normal points to.
    fn quad(&mut self, base: Vec3, du: Vec3, dv: Vec3, face: Face, key: FaceKey) {
        let mut normal = Vec3::ZERO;
        normal[face.axis] = face.sign() as f32;
        // `u x v` points along +axis, so (0, u, u+v, v) is CCW seen from +axis.
        let corners = if face.positive {
            [base, base + du, base + du + dv, base + dv]
        } else {
            [base, base + dv, base + du + dv, base + du]
        };
        let value = (key & 0xff) as Voxel;
        let slot = (key >> 8) as u8;
        let uv_ao: [u8; 4] = std::array::from_fn(|i| ((key >> (16 + 2 * i)) & 3) as u8);
        // Same corner order as `corners`: (u, v) = 00, 10, 11, 01, or
        // 00, 01, 11, 10 on negative faces.
        let ao = if face.positive {
            uv_ao
        } else {
            [uv_ao[0], uv_ao[3], uv_ao[2], uv_ao[1]]
        };
        let color = self
            .params
            .palette
            .get(value as usize)
            .unwrap_or(Rgba8::WHITE);
        let first = self.mesh.positions.len() as u32;
        self.mesh.positions.extend(corners);
        self.mesh.normals.extend([normal; 4]);
        self.mesh.colors.extend([color; 4]);
        self.mesh.materials.extend([slot; 4]);
        self.mesh.ao.extend(ao);
        self.mesh
            .indices
            .extend(quad_indices(ao).map(|i| first + i));
    }
}

/// AO of the four corners (`(u, v)` order 00, 10, 11, 01) of a face whose
/// front cell is `front`, from the 8 cells around `front` in the face's
/// plane. Reads inside `tile` (at `origin`) skip the volume's tile lookup.
fn face_ao(
    volume: &VoxelVolume,
    tile: &Tile,
    origin: IVec3,
    front: IVec3,
    u_axis: usize,
    v_axis: usize,
) -> [u8; 4] {
    let solid = |du: i32, dv: i32| {
        let mut p = front;
        p[u_axis] += du;
        p[v_axis] += dv;
        let l = p - origin;
        let value = if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(TILE_SIZE)).all() {
            tile.get(local_index(l))
        } else {
            volume.get(p)
        };
        value != EMPTY_VOXEL
    };
    // ring[dv + 1][du + 1]; the centre is the front cell itself (empty).
    let ring: [[bool; 3]; 3] = std::array::from_fn(|j| {
        std::array::from_fn(|i| (i, j) != (1, 1) && solid(i as i32 - 1, j as i32 - 1))
    });
    [(-1i32, -1i32), (1, -1), (1, 1), (-1, 1)].map(|(du, dv)| {
        let (i, j) = ((du + 1) as usize, (dv + 1) as usize);
        corner_ao(ring[1][i], ring[j][1], ring[j][i])
    })
}

/// Two CCW triangles over corners 0..4. The diagonal joins the brighter
/// pair, so a dark corner darkens only its own triangle instead of
/// streaking along the diagonal (the classic AO anisotropy fix).
fn quad_indices(ao: [u8; 4]) -> [u32; 6] {
    if ao[1] + ao[3] > ao[0] + ao[2] {
        [1, 2, 3, 1, 3, 0]
    } else {
        [0, 1, 2, 0, 2, 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    fn volume(cells: &[(IVec3, Voxel)]) -> VoxelVolume {
        let mut v = VoxelVolume::new(BoundSize::new(32, 32, 32));
        for &(p, c) in cells {
            v.set(p, c);
        }
        v
    }

    fn quads(m: &Mesh) -> usize {
        assert!(m.is_valid());
        assert_eq!(m.vertex_count(), m.triangle_count() * 2);
        m.triangle_count() / 2
    }

    fn mesh(v: &VoxelVolume) -> Mesh {
        greedy_mesh(&[(v, true)], &Palette::default())
    }

    #[test]
    fn single_voxel_is_a_cube() {
        let m = mesh(&volume(&[(IVec3::new(3, 4, 5), 9)]));
        assert_eq!(quads(&m), 6);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.vertex_count(), 24);
        let (lo, hi) = m.positions.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        assert_eq!(
            (lo, hi),
            (Vec3::new(3.0, 4.0, 5.0), Vec3::new(4.0, 5.0, 6.0))
        );
    }

    #[test]
    fn shared_face_is_culled() {
        // Different colors: no merging, so 2 x 6 - 2 internal faces.
        let m = mesh(&volume(&[(IVec3::ZERO, 1), (IVec3::X, 2)]));
        assert_eq!(quads(&m), 10);
        // Same color: the four long sides merge too.
        let m = mesh(&volume(&[(IVec3::ZERO, 1), (IVec3::X, 1)]));
        assert_eq!(quads(&m), 6);
    }

    #[test]
    fn solid_block_culls_interior() {
        let cells: Vec<(IVec3, Voxel)> = (0..27)
            .map(|i| IVec3::new(i % 3, (i / 3) % 3, i / 9))
            // Checkerboard colors so nothing merges: 6 sides x 9 faces.
            .map(|p| (p, 1 + ((p.x + p.y + p.z) % 2) as Voxel))
            .collect();
        assert_eq!(quads(&mesh(&volume(&cells))), 54);
        // Single color: one quad per side.
        let solid: Vec<_> = cells.iter().map(|&(p, _)| (p, 4)).collect();
        assert_eq!(quads(&mesh(&volume(&solid))), 6);
    }

    #[test]
    fn row_merges_into_long_quads() {
        let cells: Vec<_> = (0..4).map(|x| (IVec3::new(x, 0, 0), 5)).collect();
        let m = mesh(&volume(&cells));
        assert_eq!(quads(&m), 6);
        // The +Y face spans the whole row.
        let top: Vec<f32> = m
            .positions
            .iter()
            .zip(&m.normals)
            .filter(|(_, n)| **n == Vec3::Y)
            .map(|(p, _)| p.x)
            .collect();
        assert_eq!(top.len(), 4);
        assert_eq!(top.iter().copied().fold(f32::MAX, f32::min), 0.0);
        assert_eq!(top.iter().copied().fold(f32::MIN, f32::max), 4.0);
    }

    #[test]
    fn colors_do_not_merge() {
        let m = mesh(&volume(&[
            (IVec3::ZERO, 1),
            (IVec3::X, 2),
            (IVec3::new(2, 0, 0), 1),
        ]));
        assert_eq!(quads(&m), 3 * 4 + 2);
    }

    #[test]
    fn hidden_layers_are_skipped() {
        let a = volume(&[(IVec3::ZERO, 1)]);
        let b = volume(&[(IVec3::new(5, 5, 5), 2)]);
        let palette = Palette::default();
        let tris = |layers: &[(&VoxelVolume, bool)]| greedy_mesh(layers, &palette).triangle_count();
        assert_eq!(tris(&[(&a, true), (&b, false)]), 12);
        assert_eq!(tris(&[(&a, false), (&b, false)]), 0);
        assert_eq!(tris(&[(&a, true), (&b, true)]), 24);
    }

    #[test]
    fn vertex_colors_match_palette() {
        let palette = Palette::default();
        let m = mesh(&volume(&[(IVec3::ZERO, 12), (IVec3::new(3, 0, 0), 40)]));
        let expected = [palette.get(12).unwrap(), palette.get(40).unwrap()];
        assert!(m.colors.iter().all(|c| expected.contains(c)));
        assert!(m.colors.contains(&expected[0]) && m.colors.contains(&expected[1]));
    }

    #[test]
    fn tile_borders_and_bounds_stay_closed() {
        // Spans the x = 15/16 tile border and touches the bounds edge.
        let mut v = VoxelVolume::new(BoundSize::new(17, 1, 1));
        for x in 14..17 {
            v.set(IVec3::new(x, 0, 0), 3);
        }
        let m = mesh(&v);
        // No X-facing faces at the internal x = 15 and x = 16 planes.
        let internal = m
            .positions
            .chunks_exact(4)
            .zip(m.normals.chunks_exact(4))
            .filter(|(p, n)| n[0].x != 0.0 && p.iter().all(|q| q.x == 15.0 || q.x == 16.0))
            .count();
        assert_eq!(internal, 0);
        // Two end caps + 4 sides split at the tile border.
        assert_eq!(quads(&m), 2 + 8);
    }

    #[test]
    fn quads_wind_counter_clockwise_from_outside() {
        let m = mesh(&volume(&[(IVec3::ZERO, 1)]));
        for tri in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| m.positions[tri[i] as usize]);
            let n = m.normals[tri[0] as usize];
            assert!((b - a).cross(c - a).dot(n) > 0.0);
        }
    }

    #[test]
    fn output_is_deterministic() {
        let cells: Vec<_> = (0..40)
            .map(|i| {
                (
                    IVec3::new(i % 31, (i * 7) % 29, (i * 3) % 30),
                    1 + (i % 5) as Voxel,
                )
            })
            .collect();
        assert_eq!(mesh(&volume(&cells)), mesh(&volume(&cells)));
    }

    #[test]
    fn tile_cache_matches_full_meshing() {
        let palette = Palette::default();
        let mut v = VoxelVolume::new(BoundSize::new(48, 48, 48));
        for i in 0..400 {
            let p = IVec3::new((i * 7) % 48, (i * 13) % 20, (i * 5) % 48);
            v.set(p, 1 + (i % 4) as Voxel);
        }
        let mut cache = TileMeshCache::default();
        assert_eq!(cache.mesh(&v, &palette), mesh_volume(&v, &palette));
        let all = cache.last_rebuilt();
        assert_eq!(all, v.tile_count());

        // One voxel on a tile border: its tile and neighbours only.
        v.set(IVec3::new(16, 3, 3), 9);
        assert_eq!(cache.mesh(&v, &palette), mesh_volume(&v, &palette));
        assert!(
            cache.last_rebuilt() < all,
            "{} of {all}",
            cache.last_rebuilt()
        );

        // Emptying a tile, then an unchanged frame.
        for x in 32..48 {
            for y in 0..16 {
                for z in 0..16 {
                    v.clear_at(IVec3::new(x, y, z));
                }
            }
        }
        assert_eq!(cache.mesh(&v, &palette), mesh_volume(&v, &palette));
        assert_eq!(cache.mesh(&v, &palette), mesh_volume(&v, &palette));
        assert_eq!(cache.last_rebuilt(), 0);

        // A palette change rebuilds everything.
        let mut other = palette.clone();
        other.set(1, Rgba8::rgb(1, 2, 3));
        assert_eq!(cache.mesh(&v, &other), mesh_volume(&v, &other));
        assert_eq!(cache.last_rebuilt(), v.tile_count());
    }

    #[test]
    fn material_slots_split_quads_and_reach_the_vertices() {
        let palette = Palette::default();
        let cells: Vec<_> = (0..4).map(|x| (IVec3::new(x, 0, 0), 5)).collect();
        let v = volume(&cells);
        // One slot for all: same quads as the plain mesh, slot on every vertex.
        let uniform = mesh_volume_with(
            &v,
            &MeshParams::new(&palette).slots(MaterialSlots::Uniform(3)),
        );
        assert_eq!(quads(&uniform), 6);
        assert!(uniform.materials.iter().all(|&s| s == 3));
        // Half the row in another slot: the long sides split in two.
        let mut slots = VoxelVolume::new(v.bounds());
        slots.set(IVec3::new(2, 0, 0), 7);
        slots.set(IVec3::new(3, 0, 0), 7);
        let split = mesh_volume_with(
            &v,
            &MeshParams::new(&palette).slots(MaterialSlots::PerVoxel(&slots)),
        );
        assert_eq!(quads(&split), 2 + 4 * 2);
        assert!(split.materials.contains(&0) && split.materials.contains(&7));
        assert!(split.is_valid());
    }

    #[test]
    fn tile_cache_follows_material_slots() {
        let palette = Palette::default();
        let mut v = VoxelVolume::new(BoundSize::new(40, 20, 40));
        for i in 0..300 {
            v.set(
                IVec3::new((i * 7) % 40, (i * 3) % 20, (i * 11) % 40),
                1 + (i % 3) as Voxel,
            );
        }
        let mut slots = VoxelVolume::new(v.bounds());
        let mut cache = TileMeshCache::default();
        fn params<'a>(palette: &'a Palette, s: &'a VoxelVolume) -> MeshParams<'a> {
            MeshParams::new(palette).slots(MaterialSlots::PerVoxel(s))
        }
        let full = |s: &VoxelVolume| mesh_volume_with(&v, &params(&palette, s));
        assert_eq!(
            *cache.mesh_shared(&v, &params(&palette, &slots)),
            full(&slots)
        );
        // A slot change alone re-meshes its tile (and neighbours) only.
        slots.set(IVec3::new(7, 3, 11), 4);
        assert_eq!(
            *cache.mesh_shared(&v, &params(&palette, &slots)),
            full(&slots)
        );
        assert!(cache.last_rebuilt() < v.tile_count());
        // Switching to a uniform slot rebuilds everything.
        let uniform = MeshParams::new(&palette).slots(MaterialSlots::Uniform(2));
        assert_eq!(
            *cache.mesh_shared(&v, &uniform),
            mesh_volume_with(&v, &uniform)
        );
        assert_eq!(cache.last_rebuilt(), v.tile_count());
    }

    fn ao_mesh(v: &VoxelVolume) -> Mesh {
        mesh_volume_with(v, &MeshParams::new(&Palette::default()).ao(true))
    }

    /// AO values of the quads facing `normal` whose corners all lie on
    /// the plane `normal . p = plane`.
    fn face_ao_values(m: &Mesh, normal: Vec3, plane: f32) -> Vec<u8> {
        m.positions
            .iter()
            .zip(&m.normals)
            .zip(&m.ao)
            .filter(|((p, n), _)| **n == normal && p.dot(normal) == plane)
            .map(|(_, &a)| a)
            .collect()
    }

    #[test]
    fn single_voxel_has_no_occlusion() {
        let m = ao_mesh(&volume(&[(IVec3::new(3, 4, 5), 9)]));
        assert_eq!(quads(&m), 6);
        assert!(m.ao.iter().all(|&a| a == AO_OPEN));
        // Without AO the mesh is the plain one, every vertex open.
        assert_eq!(
            m.positions,
            mesh(&volume(&[(IVec3::new(3, 4, 5), 9)])).positions
        );
    }

    #[test]
    fn inner_corner_gets_level_two_or_less() {
        // An L: floor voxel (0,0,0) and (1,0,0) with a wall on top of (1,0,0).
        // The floor's top face meets the wall: its two corners at x = 1 have
        // the wall as one side neighbour, so they drop to level 2.
        let m = ao_mesh(&volume(&[
            (IVec3::new(0, 0, 0), 4),
            (IVec3::new(1, 0, 0), 4),
            (IVec3::new(1, 1, 0), 4),
        ]));
        let top = face_ao_values(&m, Vec3::Y, 1.0);
        assert_eq!(top.len(), 4, "one quad on the floor's free top");
        let mut sorted = top.clone();
        sorted.sort();
        assert_eq!(sorted, [2, 2, 3, 3]);
        assert!(m.is_valid());

        // A voxel in a corner of two walls and the floor: the corner vertex
        // has both sides solid, so it is fully occluded.
        let m = ao_mesh(&volume(&[
            (IVec3::new(1, 0, 1), 4),
            (IVec3::new(0, 1, 1), 4),
            (IVec3::new(1, 1, 0), 4),
        ]));
        assert!(face_ao_values(&m, Vec3::Y, 1.0).contains(&0));
    }

    #[test]
    fn flat_plane_merges_to_one_quad_with_ao() {
        let cells: Vec<_> = (0..16)
            .flat_map(|x| (0..16).map(move |z| (IVec3::new(x, 0, z), 7)))
            .collect();
        let m = ao_mesh(&volume(&cells));
        // Top and bottom are one quad each: nothing occludes a flat plane.
        assert_eq!(face_ao_values(&m, Vec3::Y, 1.0).len(), 4);
        assert_eq!(face_ao_values(&m, Vec3::NEG_Y, 0.0).len(), 4);
        assert_eq!(quads(&m), 6);
    }

    #[test]
    fn ao_splits_quads_only_where_values_differ() {
        // A wall on a floor row: the row's top face splits around the wall.
        let mut cells: Vec<_> = (0..6).map(|x| (IVec3::new(x, 0, 0), 2)).collect();
        cells.push((IVec3::new(3, 1, 0), 2));
        let plain = mesh(&volume(&cells));
        let ao = ao_mesh(&volume(&cells));
        assert!(
            quads(&ao) > quads(&plain),
            "{} vs {}",
            quads(&ao),
            quads(&plain)
        );
    }

    #[test]
    fn diagonal_joins_the_brighter_corners() {
        // One dark corner (0): the diagonal must avoid it.
        assert_eq!(quad_indices([0, 3, 3, 3]), [1, 2, 3, 1, 3, 0]);
        assert_eq!(quad_indices([3, 0, 3, 3]), [0, 1, 2, 0, 2, 3]);
        assert_eq!(quad_indices([3, 3, 3, 3]), [0, 1, 2, 0, 2, 3]);
        // Both triangulations stay counter-clockwise from outside.
        let cells = [
            (IVec3::new(1, 0, 1), 4),
            (IVec3::new(0, 1, 1), 4),
            (IVec3::new(1, 1, 0), 4),
        ];
        let m = ao_mesh(&volume(&cells));
        for tri in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| m.positions[tri[i] as usize]);
            let n = m.normals[tri[0] as usize];
            assert!((b - a).cross(c - a).dot(n) > 0.0);
        }
    }

    #[test]
    fn tile_cache_matches_full_meshing_with_ao() {
        // AO reads edge and corner neighbours: a voxel at a tile corner
        // changes the AO of up to 8 tiles, and the cache must follow.
        let palette = Palette::default();
        let params = MeshParams::new(&palette).ao(true);
        let mut v = VoxelVolume::new(BoundSize::new(96, 48, 96));
        for i in 0..600 {
            let p = IVec3::new((i * 7) % 96, (i * 13) % 40, (i * 5) % 96);
            v.set(p, 1 + (i % 3) as Voxel);
        }
        let mut cache = TileMeshCache::default();
        assert_eq!(
            *cache.mesh_shared(&v, &params),
            mesh_volume_with(&v, &params)
        );
        for p in [
            IVec3::new(16, 16, 16),
            IVec3::new(15, 15, 15),
            IVec3::new(31, 16, 0),
            IVec3::new(32, 0, 31),
        ] {
            v.set(p, 9);
            assert_eq!(
                *cache.mesh_shared(&v, &params),
                mesh_volume_with(&v, &params)
            );
            assert!(cache.last_rebuilt() < v.tile_count());
            v.clear_at(p);
            assert_eq!(
                *cache.mesh_shared(&v, &params),
                mesh_volume_with(&v, &params)
            );
        }
        // Turning AO off rebuilds everything and matches the plain mesher.
        let plain = MeshParams::new(&palette);
        assert_eq!(*cache.mesh_shared(&v, &plain), mesh_volume(&v, &palette));
        assert_eq!(cache.last_rebuilt(), v.tile_count());
    }

    #[test]
    fn unit_cube_is_valid() {
        let m = Mesh::unit_cube(Rgba8::WHITE);
        assert!(m.is_valid());
        assert_eq!(m.vertex_count(), 8);
        assert_eq!(m.triangle_count(), 12);
    }
}
