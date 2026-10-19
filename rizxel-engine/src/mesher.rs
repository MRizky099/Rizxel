//! Greedy voxel mesher and the indexed triangle mesh it produces.
//!
//! [`greedy_mesh`] turns the visible layers of a document into one mesh in
//! model space (voxel `v` occupies `[v, v + 1)`). Each 16³ tile is swept along
//! the three axes in both directions; faces whose neighbour across the face is
//! empty (or outside the bounds, so the shell is always closed) are collected
//! into a 16×16 mask per slice and merged greedily into rectangles of one
//! palette index. Quads never merge across tile borders.

use glam::{IVec3, Vec3};
use rizxel_math::volume::local_index;
use rizxel_math::{Palette, Rgba8, Tile, Voxel, VoxelVolume, EMPTY_VOXEL, TILE_SIZE};

/// Indexed triangle mesh. All per-vertex arrays have the same length.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<Rgba8>,
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
            && self.indices.len() % 3 == 0
            && self.indices.iter().all(|&i| (i as usize) < n)
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
            indices,
        }
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

/// Greedy-meshes a single volume (see the module docs).
pub fn mesh_volume(volume: &VoxelVolume, palette: &Palette) -> Mesh {
    let mut mesher = Mesher {
        mesh: Mesh::default(),
        palette,
    };
    // Sorted tiles keep the output byte-identical across runs.
    for origin in volume.sorted_tile_origins() {
        let tile = volume.tile(origin);
        for axis in 0..3 {
            for positive in [false, true] {
                mesher.sweep(volume, origin, tile, Face { axis, positive });
            }
        }
    }
    mesher.mesh
}

const N: usize = TILE_SIZE as usize;

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

struct Mesher<'p> {
    mesh: Mesh,
    palette: &'p Palette,
}

impl Mesher<'_> {
    /// Emits the faces of one tile that point along `face`.
    fn sweep(&mut self, volume: &VoxelVolume, origin: IVec3, tile: &Tile, face: Face) {
        let (axis, u_axis, v_axis) = (face.axis, face.u(), face.v());
        let mut normal = IVec3::ZERO;
        normal[axis] = face.sign();
        let mut mask = [EMPTY_VOXEL; N * N];

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
                    mask[v * N + u] = if visible { value } else { EMPTY_VOXEL };
                    any |= visible;
                }
            }
            if any {
                self.merge_mask(&mut mask, origin, face, slice);
            }
        }
    }

    /// Greedily covers the mask with rectangles of equal value and emits them.
    fn merge_mask(&mut self, mask: &mut [Voxel; N * N], origin: IVec3, face: Face, slice: usize) {
        for v in 0..N {
            let mut u = 0;
            while u < N {
                let value = mask[v * N + u];
                if value == EMPTY_VOXEL {
                    u += 1;
                    continue;
                }
                let mut w = 1;
                while u + w < N && mask[v * N + u + w] == value {
                    w += 1;
                }
                let mut h = 1;
                while v + h < N
                    && mask[(v + h) * N + u..(v + h) * N + u + w]
                        .iter()
                        .all(|&m| m == value)
                {
                    h += 1;
                }
                for row in v..v + h {
                    mask[row * N + u..row * N + u + w].fill(EMPTY_VOXEL);
                }

                let mut base = origin.as_vec3();
                base[face.axis] += slice as f32 + if face.positive { 1.0 } else { 0.0 };
                base[face.u()] += u as f32;
                base[face.v()] += v as f32;
                let mut du = Vec3::ZERO;
                du[face.u()] = w as f32;
                let mut dv = Vec3::ZERO;
                dv[face.v()] = h as f32;
                self.quad(base, du, dv, face, value);
                u += w;
            }
        }
    }

    /// Appends one quad (4 vertices, 2 triangles), counter-clockwise when seen
    /// from the side its normal points to.
    fn quad(&mut self, base: Vec3, du: Vec3, dv: Vec3, face: Face, value: Voxel) {
        let mut normal = Vec3::ZERO;
        normal[face.axis] = face.sign() as f32;
        // `u x v` points along +axis, so (0, u, u+v, v) is CCW seen from +axis.
        let corners = if face.positive {
            [base, base + du, base + du + dv, base + dv]
        } else {
            [base, base + dv, base + du + dv, base + du]
        };
        let color = self.palette.get(value as usize).unwrap_or(Rgba8::WHITE);
        let first = self.mesh.positions.len() as u32;
        self.mesh.positions.extend(corners);
        self.mesh.normals.extend([normal; 4]);
        self.mesh.colors.extend([color; 4]);
        self.mesh
            .indices
            .extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
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
    fn unit_cube_is_valid() {
        let m = Mesh::unit_cube(Rgba8::WHITE);
        assert!(m.is_valid());
        assert_eq!(m.vertex_count(), 8);
        assert_eq!(m.triangle_count(), 12);
    }
}
