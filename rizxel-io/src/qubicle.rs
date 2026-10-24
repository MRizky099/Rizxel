//! Pieces shared by the Qubicle formats (`.qb`, `.qef`, `.qbt`): a byte
//! reader, matrices in Rizxel's frame, and the conversions between
//! matrices and projects or export scenes.
//!
//! Qubicle is Y-up like Rizxel. A right-handed matrix maps straight onto
//! Rizxel cells (`position + local`); a left-handed one is mirrored on Z
//! (`z → -z - 1`), which [`QMatrix::mirrored_z`] applies.

use std::collections::HashMap;

use glam::{IVec3, UVec3};
use rizxel_math::{BoundSize, LayerKind, Palette, Rgba8, Voxel, VoxelVolume, EMPTY_VOXEL};

use crate::interchange::{bounds_for, build_project, ExportScene, ImportLayer};
use crate::{LayerData, ProjectFile, RzxlError, MAX_LAYERS, MAX_NAME_CHARS};

/// Largest matrix side accepted when reading (Rizxel's bounds limit).
pub(crate) const MAX_SIDE: u32 = BoundSize::MAX;

/// Bounds-checked little-endian reader.
pub(crate) struct ByteReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    /// Format name for error messages.
    pub format: &'static str,
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8], format: &'static str) -> Self {
        Self {
            data,
            pos: 0,
            format,
        }
    }

    pub fn invalid(&self, msg: impl Into<String>) -> RzxlError {
        RzxlError::Invalid(format!("{}: {}", self.format, msg.into()))
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], RzxlError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| self.invalid("unexpected end of data"))?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, RzxlError> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u32(&mut self) -> Result<u32, RzxlError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> Result<i32, RzxlError> {
        Ok(self.u32()? as i32)
    }

    pub fn f32(&mut self) -> Result<f32, RzxlError> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Matrix size, each side `1..=MAX_SIDE`.
    pub fn size(&mut self) -> Result<IVec3, RzxlError> {
        let s = [self.u32()?, self.u32()?, self.u32()?];
        if s.iter().any(|&v| v == 0 || v > MAX_SIDE) {
            return Err(self.invalid(format!(
                "matrix size {}×{}×{}: each side must be 1..={MAX_SIDE}",
                s[0], s[1], s[2]
            )));
        }
        Ok(UVec3::from_array(s).as_ivec3())
    }

    pub fn ivec3(&mut self) -> Result<IVec3, RzxlError> {
        Ok(IVec3::new(self.i32()?, self.i32()?, self.i32()?))
    }
}

/// Little-endian writer helpers.
pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_ivec3(out: &mut Vec<u8>, v: IVec3) {
    for c in v.to_array() {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

/// Layer names are capped at Rizxel's limit.
pub(crate) fn clamp_name(name: &str) -> String {
    name.chars().take(MAX_NAME_CHARS).collect()
}

/// One voxel matrix in Rizxel's frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QMatrix {
    pub name: String,
    /// Rizxel cell of the matrix's minimum corner.
    pub pos: IVec3,
    pub size: IVec3,
    /// Solid cells in matrix-local coordinates.
    pub cells: Vec<(IVec3, Rgba8)>,
    /// Index into the group list passed with the matrices.
    pub group: Option<usize>,
}

impl QMatrix {
    /// The matrix of a left-handed file in Rizxel's frame: Z is mirrored.
    pub fn mirrored_z(mut self) -> Self {
        self.pos.z = -self.pos.z - self.size.z;
        let depth = self.size.z;
        for (p, _) in &mut self.cells {
            p.z = depth - 1 - p.z;
        }
        self
    }
}

/// A group (Qubicle model or compound node).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QGroup {
    pub name: String,
    pub parent: Option<usize>,
}

/// Builds a project from matrices. Bounds are the union of the matrix boxes;
/// when it starts below zero or ends past the limit it is moved to start at
/// the origin. `palette` maps exact colors onto its indices (`.qef`, `.qbt`
/// color maps); otherwise colors are reduced with
/// [`rizxel_engine::build_color_map`].
pub(crate) fn project_from_matrices(
    name: &str,
    matrices: Vec<QMatrix>,
    groups: &[QGroup],
    palette: Option<Palette>,
) -> Result<ProjectFile, RzxlError> {
    if matrices.len() + groups.len() > MAX_LAYERS {
        return Err(RzxlError::Invalid(format!(
            "the file has more than {MAX_LAYERS} matrices and groups"
        )));
    }
    let boxes: Vec<(IVec3, IVec3)> = matrices
        .iter()
        .filter(|m| !m.cells.is_empty())
        .map(|m| (m.pos, m.pos + m.size))
        .collect();
    let (lo, hi) = boxes.iter().fold(
        (IVec3::splat(i32::MAX), IVec3::splat(i32::MIN)),
        |(lo, hi), &(a, b)| (lo.min(a), hi.max(b)),
    );
    let (shift, size) = if boxes.is_empty() {
        (IVec3::ZERO, IVec3::ONE)
    } else if lo.cmpge(IVec3::ZERO).all() && hi.cmple(IVec3::splat(MAX_SIDE as i32)).all() {
        (IVec3::ZERO, hi)
    } else {
        // i64: positions near i32::MAX must not overflow.
        let span = hi.as_i64vec3() - lo.as_i64vec3();
        if span.max_element() > MAX_SIDE as i64 {
            return Err(RzxlError::Invalid(format!(
                "the matrices span {}×{}×{} voxels; Rizxel supports up to {MAX_SIDE} per axis",
                span.x, span.y, span.z
            )));
        }
        (-lo, hi - lo)
    };
    let bounds = bounds_for(size.as_uvec3(), "The Qubicle model")?;

    let (palette, value): (Palette, Box<dyn Fn(Rgba8) -> Voxel>) = match palette {
        Some(p) => {
            let mut exact: HashMap<Rgba8, Voxel> = HashMap::new();
            for (i, c) in p.colors().iter().enumerate().skip(1) {
                exact.entry(Rgba8::rgb(c.r, c.g, c.b)).or_insert(i as Voxel);
            }
            let fallback = p.clone();
            (
                p,
                Box::new(move |c| {
                    exact
                        .get(&Rgba8::rgb(c.r, c.g, c.b))
                        .copied()
                        .unwrap_or_else(|| rizxel_engine::nearest(&fallback, c))
                }),
            )
        }
        None => {
            let mut counts: Vec<(Rgba8, u64)> = Vec::new();
            let mut index: HashMap<Rgba8, usize> = HashMap::new();
            for (_, c) in matrices.iter().flat_map(|m| &m.cells) {
                let c = Rgba8::rgb(c.r, c.g, c.b);
                match index.get(&c) {
                    Some(&i) => counts[i].1 += 1,
                    None => {
                        index.insert(c, counts.len());
                        counts.push((c, 1));
                    }
                }
            }
            if counts.is_empty() {
                counts.push((Rgba8::WHITE, 1));
            }
            let map = rizxel_engine::build_color_map(&counts);
            (map.palette.clone(), Box::new(move |c| map.value(c)))
        }
    };

    // Each group goes right before its first matrix (outer groups first),
    // so its place among its siblings survives; empty groups come last.
    let mut layers: Vec<ImportLayer> = Vec::new();
    let group_id = |g: usize| (g + 1) as u32;
    let mut pushed = vec![false; groups.len()];
    let mut push_groups = |layers: &mut Vec<ImportLayer>, g: Option<usize>| {
        let mut chain = Vec::new();
        let mut at = g.filter(|&g| g < groups.len());
        while let Some(x) = at.filter(|&x| !pushed[x]) {
            chain.push(x);
            // Parents come before their children in the list (no cycles).
            at = groups[x].parent.filter(|&p| p < x);
        }
        for &x in chain.iter().rev() {
            pushed[x] = true;
            let parent = groups[x].parent.filter(|&p| p < x).map(group_id);
            layers.push(ImportLayer {
                data: LayerData {
                    kind: LayerKind::Group { collapsed: false },
                    parent,
                    ..LayerData::new(group_id(x), clamp_name(&groups[x].name))
                },
                volume: None,
            });
        }
    };
    for (i, m) in matrices.iter().enumerate() {
        let mut volume = VoxelVolume::new(bounds);
        for &(p, c) in &m.cells {
            let v = value(c);
            if v != EMPTY_VOXEL {
                volume.set(m.pos + p + shift, v);
            }
        }
        let name = if m.name.is_empty() {
            format!("Matrix {}", i + 1)
        } else {
            clamp_name(&m.name)
        };
        let parent = m.group.filter(|&g| g < groups.len()).map(group_id);
        push_groups(&mut layers, m.group);
        layers.push(ImportLayer {
            data: LayerData {
                parent,
                ..LayerData::new((groups.len() + i + 1) as u32, name)
            },
            volume: Some(volume),
        });
    }
    for g in 0..groups.len() {
        push_groups(&mut layers, Some(g));
    }
    build_project(name, bounds, palette, Vec::new(), layers)
}

/// A visible, non-empty layer cut to its occupied box, for writing.
pub(crate) struct ExportMatrix<'a> {
    pub name: &'a str,
    pub pos: IVec3,
    pub size: IVec3,
    pub volume: &'a VoxelVolume,
}

impl ExportMatrix<'_> {
    /// Palette index at matrix-local `p`.
    pub fn get(&self, p: IVec3) -> Voxel {
        self.volume.get(self.pos + p)
    }
}

/// Visible layers with voxels as matrices (hidden layers are left out:
/// Qubicle files have no hidden flag).
pub(crate) fn export_matrices(scene: &ExportScene) -> Result<Vec<ExportMatrix<'_>>, RzxlError> {
    let out: Vec<ExportMatrix> = scene
        .layers
        .iter()
        .filter(|l| l.visible)
        .filter_map(|l| {
            let (lo, hi) = l.volume.occupied_bounds()?;
            Some(ExportMatrix {
                name: &l.name,
                pos: lo,
                size: hi - lo,
                volume: &l.volume,
            })
        })
        .collect();
    if out.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    Ok(out)
}

/// Opaque palette color of `v`.
pub(crate) fn color_of(palette: &Palette, v: Voxel) -> Rgba8 {
    let c = palette.get(v as usize).unwrap_or(Rgba8::WHITE);
    Rgba8::rgb(c.r, c.g, c.b)
}

/// Face mask of the voxel at `p`: bit 0 set for any voxel, then one bit
/// per open side (Qubicle's order: left, right, top, bottom, front, back).
pub(crate) fn face_mask(volume: &VoxelVolume, p: IVec3) -> u32 {
    let open = |d: IVec3| volume.get(p + d) == EMPTY_VOXEL;
    let sides = [
        IVec3::NEG_X,
        IVec3::X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];
    sides
        .iter()
        .enumerate()
        .fold(1, |m, (i, &d)| if open(d) { m | 2 << i } else { m })
}

/// Name bytes cut to `max` bytes on a character boundary.
pub(crate) fn name_bytes(name: &str, max: usize) -> &[u8] {
    let mut end = name.len().min(max);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name.as_bytes()[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(pos: IVec3, size: IVec3, cells: &[(IVec3, Rgba8)]) -> QMatrix {
        QMatrix {
            name: "m".into(),
            pos,
            size,
            cells: cells.to_vec(),
            group: None,
        }
    }

    #[test]
    fn bounds_keep_positive_positions_and_shift_negative_ones() {
        let red = Rgba8::rgb(255, 0, 0);
        let p = project_from_matrices(
            "t",
            vec![matrix(
                IVec3::new(2, 0, 1),
                IVec3::ONE,
                &[(IVec3::ZERO, red)],
            )],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(p.bounds, BoundSize::new(3, 1, 2));
        let p = project_from_matrices(
            "t",
            vec![
                matrix(IVec3::new(-5, 0, 0), IVec3::ONE, &[(IVec3::ZERO, red)]),
                matrix(IVec3::new(5, 0, 0), IVec3::ONE, &[(IVec3::ZERO, red)]),
            ],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(p.bounds, BoundSize::new(11, 1, 1));
        let far = project_from_matrices(
            "t",
            vec![
                matrix(IVec3::splat(i32::MIN), IVec3::ONE, &[(IVec3::ZERO, red)]),
                matrix(
                    IVec3::splat(i32::MAX - 1),
                    IVec3::ONE,
                    &[(IVec3::ZERO, red)],
                ),
            ],
            &[],
            None,
        );
        assert!(far.unwrap_err().to_string().contains("up to 256"));
    }

    #[test]
    fn left_handed_matrices_mirror_z() {
        let m = matrix(
            IVec3::new(0, 0, 2),
            IVec3::new(1, 1, 3),
            &[(IVec3::new(0, 0, 0), Rgba8::WHITE)],
        )
        .mirrored_z();
        assert_eq!(m.pos.z, -5);
        assert_eq!(m.cells[0].0.z, 2);
        // Cell z = 2 (left-handed) -> -3 (right-handed) = -5 + 2.
        assert_eq!(m.pos.z + m.cells[0].0.z, -2 - 1);
    }

    #[test]
    fn names_are_cut_on_char_boundaries() {
        assert_eq!(name_bytes("héllo", 2), b"h");
        assert_eq!(name_bytes("abc", 255), b"abc");
    }
}
