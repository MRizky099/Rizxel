//! Meshes of the render-only voxel shapes (Phase 11b): every voxel as a
//! (rounded) box, a brick with a stud on each top face that has no voxel
//! above, a sphere or a vertical cylinder. Used by the raster preview of
//! Render mode and by *Export Shaped Mesh*; the voxels never change.
//!
//! Boxes and bricks at full cell size cull the faces between two solid
//! voxels, and the edges along such faces stay sharp, so touching bricks
//! form one closed surface without hidden geometry. Positions are in voxel
//! space (voxel `v` fills `v..v + 1`); triangles wind counter-clockwise
//! seen from outside, like [`crate::greedy_mesh`]'s.

use std::collections::{HashMap, HashSet};

use glam::{IVec3, Vec3};
use rizxel_math::render::{BRICK_MIN_BEVEL, ROUNDED_BEVEL, STUD_DIAMETER, STUD_HEIGHT};
use rizxel_math::{Palette, Rgba8, VoxelShape, VoxelVolume, EMPTY_VOXEL};

use crate::Mesh;

/// Segment counts of the curved parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl ShapeQuality {
    pub const ALL: [ShapeQuality; 3] =
        [ShapeQuality::Low, ShapeQuality::Medium, ShapeQuality::High];

    pub const fn label(self) -> &'static str {
        match self {
            ShapeQuality::Low => "Low",
            ShapeQuality::Medium => "Medium",
            ShapeQuality::High => "High",
        }
    }

    /// Segments per rounded edge (a quarter circle).
    pub const fn bevel_segments(self) -> u32 {
        match self {
            ShapeQuality::Low => 1,
            ShapeQuality::Medium => 2,
            ShapeQuality::High => 3,
        }
    }

    /// Segments around studs and cylinders.
    pub const fn round_segments(self) -> u32 {
        match self {
            ShapeQuality::Low => 8,
            ShapeQuality::Medium => 16,
            ShapeQuality::High => 32,
        }
    }

    /// Segments around spheres.
    pub const fn sphere_segments(self) -> u32 {
        match self {
            ShapeQuality::Low => 8,
            ShapeQuality::Medium => 16,
            ShapeQuality::High => 24,
        }
    }
}

/// What to build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeMeshOptions {
    pub shape: VoxelShape,
    /// Shape size inside its cell (`0.5..=1`).
    pub cell: f32,
    /// Edge radius of cubes and bricks, in voxels.
    pub bevel: f32,
    pub quality: ShapeQuality,
    /// Studs on bricks.
    pub studs: bool,
}

impl ShapeMeshOptions {
    pub fn new(shape: VoxelShape) -> Self {
        Self {
            shape,
            cell: 1.0,
            bevel: 0.0,
            quality: ShapeQuality::default(),
            studs: true,
        }
    }

    /// Edge radius the shape is built with, before the cell scale.
    fn radius(&self) -> f32 {
        match self.shape {
            VoxelShape::Rounded => ROUNDED_BEVEL,
            VoxelShape::Brick => self.bevel.max(BRICK_MIN_BEVEL),
            _ => self.bevel,
        }
        .clamp(0.0, 0.5)
    }

    fn cell(&self) -> f32 {
        if self.cell.is_finite() {
            self.cell.clamp(0.5, 1.0)
        } else {
            1.0
        }
    }
}

/// Collects triangles, or only counts them (same code path, so an
/// estimate always matches the export).
struct Builder {
    mesh: Mesh,
    count_only: bool,
    triangles: usize,
    color: Rgba8,
    slot: u8,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, n: Vec3) -> u32 {
        if self.count_only {
            return 0;
        }
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        self.mesh.colors.push(self.color);
        self.mesh.materials.push(self.slot);
        (self.mesh.positions.len() - 1) as u32
    }

    /// Triangle `a b c` facing `outward`; degenerate ones are skipped.
    fn triangle(&mut self, v: [(Vec3, Vec3); 3], outward: Vec3) {
        let [(a, _), (b, _), (c, _)] = v;
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-14 {
            return;
        }
        self.triangles += 1;
        if self.count_only {
            return;
        }
        let order = if n.dot(outward) >= 0.0 {
            [0, 1, 2]
        } else {
            [0, 2, 1]
        };
        for i in order {
            let (p, normal) = v[i];
            let index = self.vertex(p, normal);
            self.mesh.indices.push(index);
        }
    }

    fn quad(&mut self, v: [(Vec3, Vec3); 4], outward: Vec3) {
        self.triangle([v[0], v[1], v[2]], outward);
        self.triangle([v[0], v[2], v[3]], outward);
    }
}

/// Coordinates along one in-plane axis of a box face: the rounded rim at
/// each end (`seg` steps of equal angle, `-h..lo` and `hi..h`) and the
/// flat part between.
fn rim_coords(h: f32, lo: f32, hi: f32, seg: u32) -> Vec<f32> {
    let seg = seg.max(1);
    // Angle from the flat part: 0 at the inner edge, 45° at the box edge
    // (the other 45° belongs to the neighbouring face).
    let tan = |k: u32| (std::f32::consts::FRAC_PI_4 * k as f32 / seg as f32).tan();
    let mut out: Vec<f32> = (0..=seg).rev().map(|k| lo - (lo + h) * tan(k)).collect();
    out.extend((0..=seg).map(|k| hi + (h - hi) * tan(k)));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    out
}

fn axis(i: usize) -> Vec3 {
    let mut v = Vec3::ZERO;
    v[i] = 1.0;
    v
}

/// Box of half size `h` around `center` with edge radius `r`, except the
/// sides in `sharp` (`[-x, +x, -y, +y, -z, +z]`) whose edges stay square,
/// skipping the faces in `cull`.
fn rounded_box(
    b: &mut Builder,
    center: Vec3,
    h: f32,
    r: f32,
    seg: u32,
    sharp: [bool; 6],
    cull: [bool; 6],
) {
    let r = r.min(h);
    let lo = Vec3::from_array([0, 1, 2].map(|a| -h + if sharp[a * 2] { 0.0 } else { r }));
    let hi = Vec3::from_array([0, 1, 2].map(|a| h - if sharp[a * 2 + 1] { 0.0 } else { r }));
    // Maps a point of the box surface onto the rounded surface.
    let shape = |p: Vec3, face: Vec3| -> (Vec3, Vec3) {
        let q = p.clamp(lo, hi);
        let d = p - q;
        let n = d.try_normalize().unwrap_or(face);
        let len = d.length();
        let on = if len > 1e-9 { q + n * r.min(len) } else { p };
        // Square edges keep the box surface.
        let pos = if r <= 0.0 { p } else { on };
        (center + pos, if r <= 0.0 { face } else { n })
    };
    for a in 0..3 {
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let us = rim_coords(h, lo[u], hi[u], seg);
        let vs = rim_coords(h, lo[v], hi[v], seg);
        for sign in [-1.0f32, 1.0] {
            let face = if sign < 0.0 { a * 2 } else { a * 2 + 1 };
            if cull[face] {
                continue;
            }
            let normal = axis(a) * sign;
            let point = |x: f32, y: f32| {
                let mut p = Vec3::ZERO;
                p[a] = h * sign;
                p[u] = x;
                p[v] = y;
                shape(p, normal)
            };
            for i in 0..us.len().saturating_sub(1) {
                for j in 0..vs.len().saturating_sub(1) {
                    let quad = [
                        point(us[i], vs[j]),
                        point(us[i + 1], vs[j]),
                        point(us[i + 1], vs[j + 1]),
                        point(us[i], vs[j + 1]),
                    ];
                    b.quad(quad, normal);
                }
            }
        }
    }
}

/// Vertical cylinder from `y0` to `y1`; `bottom` adds the lower cap.
fn cylinder(b: &mut Builder, center: Vec3, radius: f32, y0: f32, y1: f32, seg: u32, bottom: bool) {
    let seg = seg.max(3);
    let ring = |k: u32| {
        let a = std::f32::consts::TAU * k as f32 / seg as f32;
        Vec3::new(a.cos(), 0.0, a.sin())
    };
    for k in 0..seg {
        let (d0, d1) = (ring(k), ring(k + 1));
        let p = |d: Vec3, y: f32| (Vec3::new(center.x, y, center.z) + d * radius, d);
        let side = [p(d0, y0), p(d1, y0), p(d1, y1), p(d0, y1)];
        b.quad(side, (d0 + d1) * 0.5);
        let top = Vec3::new(center.x, y1, center.z);
        b.triangle(
            [
                (top, Vec3::Y),
                (top + d0 * radius, Vec3::Y),
                (top + d1 * radius, Vec3::Y),
            ],
            Vec3::Y,
        );
        if bottom {
            let low = Vec3::new(center.x, y0, center.z);
            b.triangle(
                [
                    (low, Vec3::NEG_Y),
                    (low + d0 * radius, Vec3::NEG_Y),
                    (low + d1 * radius, Vec3::NEG_Y),
                ],
                Vec3::NEG_Y,
            );
        }
    }
}

/// UV sphere.
fn sphere(b: &mut Builder, center: Vec3, radius: f32, seg: u32) {
    let seg = seg.max(4);
    let rings = (seg / 2).max(2);
    let dir = |i: u32, k: u32| {
        let theta = std::f32::consts::PI * i as f32 / rings as f32;
        let phi = std::f32::consts::TAU * k as f32 / seg as f32;
        Vec3::new(
            theta.sin() * phi.cos(),
            theta.cos(),
            theta.sin() * phi.sin(),
        )
    };
    for i in 0..rings {
        for k in 0..seg {
            let ds = [dir(i, k), dir(i + 1, k), dir(i + 1, k + 1), dir(i, k + 1)];
            let v = ds.map(|d| (center + d * radius, d));
            let outward = (ds[0] + ds[2]).normalize_or(Vec3::Y);
            b.quad(v, outward);
        }
    }
}

/// Merged visible voxels: color and material slot (later layers win).
struct Merged {
    colors: VoxelVolume,
    slots: VoxelVolume,
}

impl Merged {
    fn new(layers: &[(&VoxelVolume, u8)]) -> Option<Self> {
        let bounds = layers.first()?.0.bounds();
        let mut colors = VoxelVolume::new(bounds);
        let mut slots = VoxelVolume::new(bounds);
        for (volume, slot) in layers {
            for (p, v) in volume.iter() {
                if v != EMPTY_VOXEL {
                    colors.set(p, v);
                    // Stored + 1 so slot 0 is not "empty".
                    slots.set(p, slot.saturating_add(1));
                }
            }
        }
        Some(Self { colors, slots })
    }

    fn solid(&self, p: IVec3) -> bool {
        self.colors.get(p) != EMPTY_VOXEL
    }
}

/// The six side directions, in the order of the `sharp` / `cull` arrays.
const SIDES: [IVec3; 6] = [
    IVec3::NEG_X,
    IVec3::X,
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
];

/// Sides (bit `k` = [`SIDES`]`[k]`) each voxel draws square instead of
/// rounded. A side's rounded edges show on the planes it shares with its
/// neighbours across the other two axes, so those neighbours must round it
/// alike or the surface cracks there. Per side direction, the voxels of a
/// flat patch (connected across the other two axes in one layer) round it
/// only when none of them has a voxel on that side.
fn square_sides(merged: &Merged) -> HashMap<IVec3, u8> {
    let mut out: HashMap<IVec3, u8> = HashMap::new();
    let solid: Vec<IVec3> = merged
        .colors
        .iter()
        .filter(|(_, v)| *v != EMPTY_VOXEL)
        .map(|(p, _)| p)
        .collect();
    for (k, side) in SIDES.iter().enumerate() {
        let axis = (0..3).find(|&a| side[a] != 0).unwrap_or(0);
        let steps: Vec<IVec3> = SIDES.iter().copied().filter(|d| d[axis] == 0).collect();
        let mut seen: HashSet<IVec3> = HashSet::new();
        for &start in &solid {
            if !seen.insert(start) {
                continue;
            }
            // Flood the patch, noting whether any voxel is covered on `side`.
            let mut patch = vec![start];
            let mut stack = vec![start];
            let mut covered = false;
            while let Some(p) = stack.pop() {
                covered |= merged.solid(p + *side);
                for d in &steps {
                    let q = p + *d;
                    if merged.solid(q) && seen.insert(q) {
                        patch.push(q);
                        stack.push(q);
                    }
                }
            }
            if covered {
                for p in patch {
                    *out.entry(p).or_default() |= 1 << k;
                }
            }
        }
    }
    out
}

fn build(
    layers: &[(&VoxelVolume, u8)],
    palette: &Palette,
    opts: &ShapeMeshOptions,
    count_only: bool,
) -> (Mesh, usize) {
    let mut b = Builder {
        mesh: Mesh::default(),
        count_only,
        triangles: 0,
        color: Rgba8::WHITE,
        slot: 0,
    };
    let Some(merged) = Merged::new(layers) else {
        return (b.mesh, 0);
    };
    let s = opts.cell();
    let h = 0.5 * s;
    let full = s >= 0.999;
    let q = opts.quality;
    let r = opts.radius() * s;
    let joins = full && matches!(opts.shape, VoxelShape::Cube | VoxelShape::Brick);
    let square = if joins {
        square_sides(&merged)
    } else {
        HashMap::new()
    };
    for (p, v) in merged.colors.iter() {
        if v == EMPTY_VOXEL {
            continue;
        }
        b.color = palette.get(v as usize).unwrap_or(Rgba8::WHITE);
        b.slot = merged.slots.get(p).saturating_sub(1);
        let center = p.as_vec3() + Vec3::splat(0.5);
        match opts.shape {
            VoxelShape::Cube | VoxelShape::Brick | VoxelShape::Rounded => {
                let neighbour = SIDES.map(|d| joins && merged.solid(p + d));
                let bits = square.get(&p).copied().unwrap_or(0);
                let sharp: [bool; 6] = std::array::from_fn(|k| bits & (1 << k) != 0);
                let r = if opts.shape == VoxelShape::Cube {
                    opts.bevel.clamp(0.0, 0.5) * s
                } else {
                    r
                };
                rounded_box(&mut b, center, h, r, q.bevel_segments(), sharp, neighbour);
                if opts.shape == VoxelShape::Brick && opts.studs && !merged.solid(p + IVec3::Y) {
                    let top = center.y + h;
                    cylinder(
                        &mut b,
                        center,
                        0.5 * STUD_DIAMETER * s,
                        top,
                        top + STUD_HEIGHT * s,
                        q.round_segments(),
                        // Capped below too, so every stud is a closed shell (STL).
                        true,
                    );
                }
            }
            VoxelShape::Sphere => sphere(&mut b, center, h, q.sphere_segments()),
            VoxelShape::Cylinder => cylinder(
                &mut b,
                center,
                h,
                center.y - h,
                center.y + h,
                q.round_segments(),
                true,
            ),
        }
    }
    let n = b.triangles;
    (b.mesh, n)
}

/// Mesh of `layers` (each with its material slot) drawn as `opts.shape`.
pub fn shape_mesh(
    layers: &[(&VoxelVolume, u8)],
    palette: &Palette,
    opts: &ShapeMeshOptions,
) -> Mesh {
    build(layers, palette, opts, false).0
}

/// Triangles [`shape_mesh`] would build, without building them.
pub fn shape_triangle_count(layers: &[(&VoxelVolume, u8)], opts: &ShapeMeshOptions) -> usize {
    build(layers, &Palette::default(), opts, true).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;
    use std::collections::HashMap;

    fn volume(cells: &[IVec3]) -> VoxelVolume {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        for &c in cells {
            v.set(c, 3);
        }
        v
    }

    /// Every edge (by rounded positions) is used by an even number of
    /// triangles: no cracks.
    fn is_closed(mesh: &Mesh) -> bool {
        let key = |p: Vec3| (p * 1e4).round().as_ivec3();
        let mut edges: HashMap<(IVec3, IVec3), i32> = HashMap::new();
        for t in mesh.indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (a, b) = (
                    key(mesh.positions[a as usize]),
                    key(mesh.positions[b as usize]),
                );
                let k = if (a.x, a.y, a.z) < (b.x, b.y, b.z) {
                    (a, b)
                } else {
                    (b, a)
                };
                *edges.entry(k).or_default() += 1;
            }
        }
        // Bricks touching only along an edge share it four times: closed.
        !edges.is_empty() && edges.values().all(|&n| n % 2 == 0)
    }

    fn studs(mesh: &Mesh) -> usize {
        // Stud caps: upward triangles above the brick tops.
        let tops: std::collections::BTreeSet<(i32, i32, i32)> = mesh
            .positions
            .iter()
            .zip(&mesh.normals)
            .filter(|(p, n)| n.y > 0.99 && p.y.fract() > 0.1)
            .map(|(p, _)| (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32))
            .collect();
        tops.len()
    }

    #[test]
    fn studs_only_on_tops_with_nothing_above() {
        // A column of two and a single voxel: two studs.
        let v = volume(&[
            IVec3::new(0, 0, 0),
            IVec3::new(0, 1, 0),
            IVec3::new(2, 0, 0),
        ]);
        let mesh = shape_mesh(
            &[(&v, 0)],
            &Palette::default(),
            &ShapeMeshOptions::new(VoxelShape::Brick),
        );
        assert_eq!(studs(&mesh), 2);
        let mut no_studs = ShapeMeshOptions::new(VoxelShape::Brick);
        no_studs.studs = false;
        let mesh = shape_mesh(&[(&v, 0)], &Palette::default(), &no_studs);
        assert_eq!(studs(&mesh), 0);
    }

    #[test]
    fn two_bricks_share_no_faces() {
        let v = volume(&[IVec3::new(1, 0, 1), IVec3::new(2, 0, 1)]);
        let opts = ShapeMeshOptions::new(VoxelShape::Brick);
        let mesh = shape_mesh(&[(&v, 2)], &Palette::default(), &opts);
        assert_eq!(studs(&mesh), 2);
        // No triangle lies in the shared plane x = 2 facing ±X.
        for t in mesh.indices.chunks_exact(3) {
            let ps = [0, 1, 2].map(|i| mesh.positions[t[i] as usize]);
            let in_plane = ps.iter().all(|p| (p.x - 2.0).abs() < 1e-5);
            assert!(!in_plane, "a face between the bricks was kept");
        }
        assert!(mesh.materials.iter().all(|&m| m == 2));
        // The two blocks without studs form one closed surface.
        let mut plain = opts;
        plain.studs = false;
        let mesh = shape_mesh(&[(&v, 0)], &Palette::default(), &plain);
        assert!(is_closed(&mesh));
    }

    #[test]
    fn stepped_bricks_form_a_closed_surface() {
        // Steps, an overhang and a lone column: neighbours whose sides are
        // covered differently must still meet without cracks.
        let v = volume(&[
            IVec3::new(1, 0, 1),
            IVec3::new(2, 0, 1),
            IVec3::new(3, 0, 1),
            IVec3::new(2, 1, 1),
            IVec3::new(2, 0, 2),
            IVec3::new(3, 1, 2),
            IVec3::new(3, 0, 2),
            IVec3::new(5, 0, 5),
            IVec3::new(5, 1, 5),
        ]);
        for (shape, studs) in [
            (VoxelShape::Brick, false),
            (VoxelShape::Brick, true),
            (VoxelShape::Cube, false),
        ] {
            let opts = ShapeMeshOptions {
                studs,
                bevel: 0.1,
                ..ShapeMeshOptions::new(shape)
            };
            let mesh = shape_mesh(&[(&v, 0)], &Palette::default(), &opts);
            assert!(is_closed(&mesh), "{shape:?} cracks between bricks");
        }
    }

    #[test]
    fn spheres_cylinders_and_rounded_boxes_are_closed() {
        let v = volume(&[IVec3::new(3, 3, 3)]);
        for shape in [
            VoxelShape::Sphere,
            VoxelShape::Cylinder,
            VoxelShape::Rounded,
        ] {
            for quality in ShapeQuality::ALL {
                let opts = ShapeMeshOptions {
                    quality,
                    cell: 0.8,
                    ..ShapeMeshOptions::new(shape)
                };
                let mesh = shape_mesh(&[(&v, 0)], &Palette::default(), &opts);
                assert!(mesh.is_valid());
                assert!(is_closed(&mesh), "{shape:?} {quality:?} is open");
                // Inside its cell.
                assert!(mesh
                    .positions
                    .iter()
                    .all(|p| p.cmpge(Vec3::splat(3.0)).all() && p.cmple(Vec3::splat(4.0)).all()));
            }
        }
    }

    #[test]
    fn count_matches_the_mesh() {
        let v = volume(&[
            IVec3::new(0, 0, 0),
            IVec3::new(1, 0, 0),
            IVec3::new(1, 1, 0),
            IVec3::new(4, 4, 4),
        ]);
        for shape in VoxelShape::ALL {
            let opts = ShapeMeshOptions {
                bevel: 0.1,
                ..ShapeMeshOptions::new(shape)
            };
            let mesh = shape_mesh(&[(&v, 0)], &Palette::default(), &opts);
            assert_eq!(
                shape_triangle_count(&[(&v, 0)], &opts),
                mesh.triangle_count(),
                "{shape:?}"
            );
        }
        // Plain cubes at full size: 6 faces per lone voxel.
        let lone = volume(&[IVec3::ZERO]);
        let cube = ShapeMeshOptions::new(VoxelShape::Cube);
        assert_eq!(shape_triangle_count(&[(&lone, 0)], &cube), 12);
    }

    #[test]
    fn later_layers_win() {
        let a = volume(&[IVec3::ZERO]);
        let mut b = VoxelVolume::new(BoundSize::new(8, 8, 8));
        b.set(IVec3::ZERO, 9);
        let palette = Palette::default();
        let mesh = shape_mesh(
            &[(&a, 1), (&b, 4)],
            &palette,
            &ShapeMeshOptions::new(VoxelShape::Sphere),
        );
        assert!(mesh.materials.iter().all(|&m| m == 4));
        assert!(mesh.colors.iter().all(|&c| Some(c) == palette.get(9)));
    }
}
