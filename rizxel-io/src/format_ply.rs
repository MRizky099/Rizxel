//! Stanford PLY export (binary little-endian) and import.
//!
//! - **Export**: the merged visible layers as the same greedy quads as the
//!   OBJ export. Each quad has 4 vertices (`x y z` and `nx ny nz` as
//!   `float`, `red green blue` as `uchar`) and one 4-sided face, wound
//!   counter-clockwise from outside. Z-up like MagicaVoxel's PLY export and
//!   Blender, placed by the pivot (default: centered on X/Y, standing on
//!   Z = 0), 1 voxel = `scale` units.
//! - **Import**: ASCII, binary little- and big-endian. Vertices need `x y z`;
//!   `red green blue` (integer 0..255 or float 0..1) color them. Faces are
//!   `vertex_indices` lists, split into fans. Other elements and
//!   properties are skipped. The triangles come back Z-up for
//!   [`crate::mesh_import`].

use glam::Vec3;
use rizxel_engine::{greedy_quads, Triangle};
use rizxel_math::{Rgba8, VoxelVolume};

use crate::RzxlError;

pub const PLY_EXTENSION: &str = "ply";
const MAX_VERTICES: usize = 20_000_000;
const MAX_TRIANGLES: usize = 40_000_000;
const MAX_POLYGON: usize = 256;
const MAX_HEADER: usize = 64 * 1024;

/// Rizxel (Y-up) point -> PLY Z-up point (a rotation, not a mirror).
fn to_zup(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

/// Encodes the voxels of `volume` as binary PLY. `origin` is the Rizxel
/// point that becomes `(0, 0, 0)` (see [`crate::mesh_export::Pivot`]).
pub fn write_ply(
    volume: &VoxelVolume,
    palette: &rizxel_math::Palette,
    scale: f32,
    origin: Vec3,
) -> Result<Vec<u8>, RzxlError> {
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        crate::DEFAULT_OBJ_SCALE
    };
    let origin = if origin.is_finite() {
        origin
    } else {
        Vec3::ZERO
    };
    let quads = greedy_quads(volume);
    let header = format!(
        "ply\nformat binary_little_endian 1.0\ncomment Rizxel {} (MRizky099)\n\
         element vertex {}\nproperty float x\nproperty float y\nproperty float z\n\
         property float nx\nproperty float ny\nproperty float nz\n\
         property uchar red\nproperty uchar green\nproperty uchar blue\n\
         element face {}\nproperty list uchar uint vertex_indices\nend_header\n",
        env!("CARGO_PKG_VERSION"),
        quads.len() * 4,
        quads.len()
    );
    let mut out = Vec::with_capacity(header.len() + quads.len() * (4 * 27 + 17));
    out.extend_from_slice(header.as_bytes());
    let put = |out: &mut Vec<u8>, v: Vec3| {
        for c in v.to_array() {
            out.extend_from_slice(&(c + 0.0).to_le_bytes());
        }
    };
    for q in &quads {
        let mut n = Vec3::ZERO;
        n[q.axis] = if q.positive { 1.0 } else { -1.0 };
        let c = palette.get(q.value as usize).unwrap_or(Rgba8::WHITE);
        for p in q.corners() {
            put(&mut out, to_zup((p.as_vec3() - origin) * scale));
            put(&mut out, to_zup(n));
            out.extend_from_slice(&[c.r, c.g, c.b]);
        }
    }
    for (i, q) in quads.iter().enumerate() {
        let b = (i * 4) as u32;
        // Corners A B C D; A B D C runs around the quad.
        let order = if q.abc_is_ccw() {
            [b, b + 1, b + 3, b + 2]
        } else {
            [b + 2, b + 3, b + 1, b]
        };
        out.push(4);
        for k in order {
            out.extend_from_slice(&k.to_le_bytes());
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ import

fn invalid(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("PLY: {}", msg.into()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(s: &str) -> Option<Scalar> {
        Some(match s {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }

    fn is_float(self) -> bool {
        matches!(self, Scalar::F32 | Scalar::F64)
    }
}

#[derive(Debug, Clone)]
struct Property {
    name: String,
    kind: Scalar,
    /// Count type for list properties.
    list: Option<Scalar>,
}

#[derive(Debug, Clone)]
struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

#[derive(Clone, Copy, PartialEq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

/// Reads values from the body, ASCII tokens or binary.
struct Body<'a> {
    data: &'a [u8],
    pos: usize,
    encoding: Encoding,
}

impl Body<'_> {
    fn token(&mut self) -> Result<&str, RzxlError> {
        while self.pos < self.data.len() && self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let start = self.pos;
        while self.pos < self.data.len() && !self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(invalid("unexpected end of data"));
        }
        std::str::from_utf8(&self.data[start..self.pos]).map_err(|_| invalid("bad number"))
    }

    fn value(&mut self, kind: Scalar) -> Result<f64, RzxlError> {
        if self.encoding == Encoding::Ascii {
            return self
                .token()?
                .parse::<f64>()
                .map_err(|_| invalid("bad number"));
        }
        let n = kind.size();
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| invalid("unexpected end of data"))?;
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(&self.data[self.pos..end]);
        if self.encoding == Encoding::Big {
            b[..n].reverse();
        }
        self.pos = end;
        Ok(match kind {
            Scalar::I8 => b[0] as i8 as f64,
            Scalar::U8 => b[0] as f64,
            Scalar::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::U16 => u16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F64 => f64::from_le_bytes(b),
        })
    }
}

/// Parses a PLY file into Z-up triangles; uncolored ones get `color`.
pub fn read_ply(bytes: &[u8], color: Rgba8) -> Result<Vec<Triangle>, RzxlError> {
    let end = bytes
        .windows(10)
        .take(MAX_HEADER)
        .position(|w| w == b"end_header")
        .ok_or_else(|| invalid("not a PLY file (no end_header)"))?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid("bad header"))?;
    let mut body_start = end + 10;
    // The header line ends with LF or CRLF.
    while body_start < bytes.len() && bytes[body_start] != b'\n' {
        body_start += 1;
    }
    body_start = (body_start + 1).min(bytes.len());

    let mut lines = header.lines().map(str::trim);
    if lines.next() != Some("ply") {
        return Err(invalid("not a PLY file"));
    }
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in lines {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            ["format", f, ..] => {
                encoding = Some(match *f {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    _ => return Err(invalid(format!("unknown format {f}"))),
                })
            }
            ["element", name, count] => elements.push(Element {
                name: (*name).to_owned(),
                count: count
                    .parse()
                    .map_err(|_| invalid(format!("bad element count {count}")))?,
                properties: Vec::new(),
            }),
            ["property", "list", c, k, name] => {
                let (Some(c), Some(k)) = (Scalar::parse(c), Scalar::parse(k)) else {
                    return Err(invalid(format!("unknown property type in {line:?}")));
                };
                elements
                    .last_mut()
                    .ok_or_else(|| invalid("property before any element"))?
                    .properties
                    .push(Property {
                        name: (*name).to_owned(),
                        kind: k,
                        list: Some(c),
                    });
            }
            ["property", k, name] => {
                let kind = Scalar::parse(k)
                    .ok_or_else(|| invalid(format!("unknown property type {k}")))?;
                elements
                    .last_mut()
                    .ok_or_else(|| invalid("property before any element"))?
                    .properties
                    .push(Property {
                        name: (*name).to_owned(),
                        kind,
                        list: None,
                    });
            }
            _ => {}
        }
    }
    let encoding = encoding.ok_or_else(|| invalid("missing format line"))?;
    let mut body = Body {
        data: &bytes[body_start..],
        pos: 0,
        encoding,
    };

    let mut positions: Vec<Vec3> = Vec::new();
    let mut colors: Vec<Option<Rgba8>> = Vec::new();
    let mut triangles: Vec<Triangle> = Vec::new();
    for el in &elements {
        let is_vertex = el.name == "vertex";
        let is_face = el.name == "face";
        if is_vertex && el.count > MAX_VERTICES {
            return Err(invalid("too many vertices"));
        }
        // Binary elements of fixed size are bounds-checked up front.
        let fixed: Option<usize> = el
            .properties
            .iter()
            .map(|p| p.list.is_none().then_some(p.kind.size()))
            .sum();
        if let (Some(row), true) = (fixed, encoding != Encoding::Ascii) {
            if row
                .checked_mul(el.count)
                .is_none_or(|n| n > body.data.len() - body.pos)
            {
                return Err(invalid(format!("the {} data is truncated", el.name)));
            }
        }
        let find = |n: &str| el.properties.iter().position(|p| p.name == n);
        let xyz = [find("x"), find("y"), find("z")];
        let rgb = [find("red"), find("green"), find("blue")];
        let indices = find("vertex_indices").or_else(|| find("vertex_index"));
        if is_vertex {
            positions.reserve(el.count.min(body.data.len()));
        }
        for _ in 0..el.count {
            let mut scalars: Vec<f64> = vec![0.0; el.properties.len()];
            let mut list: Vec<f64> = Vec::new();
            for (k, p) in el.properties.iter().enumerate() {
                match p.list {
                    None => scalars[k] = body.value(p.kind)?,
                    Some(c) => {
                        let n = body.value(c)?;
                        if !(0.0..=MAX_POLYGON as f64).contains(&n) {
                            return Err(invalid(format!("polygon of {n} corners")));
                        }
                        let keep = Some(k) == indices;
                        for _ in 0..n as usize {
                            let v = body.value(p.kind)?;
                            if keep {
                                list.push(v);
                            }
                        }
                    }
                }
            }
            if is_vertex {
                let get = |i: Option<usize>| i.map_or(0.0, |i| scalars[i]) as f32;
                positions.push(Vec3::new(get(xyz[0]), get(xyz[1]), get(xyz[2])));
                colors.push(match rgb {
                    [Some(r), Some(g), Some(b)] => {
                        let float = el.properties[r].kind.is_float();
                        let ch = |v: f64| {
                            let v = if float { v * 255.0 } else { v };
                            v.round().clamp(0.0, 255.0) as u8
                        };
                        Some(Rgba8::rgb(ch(scalars[r]), ch(scalars[g]), ch(scalars[b])))
                    }
                    _ => None,
                });
            } else if is_face && list.len() >= 3 {
                let corner = |v: f64| -> Result<usize, RzxlError> {
                    (v >= 0.0 && (v as usize) < positions.len())
                        .then_some(v as usize)
                        .ok_or_else(|| invalid(format!("bad vertex index {v}")))
                };
                let c: Vec<usize> = list.iter().map(|&v| corner(v)).collect::<Result<_, _>>()?;
                for k in 1..c.len() - 1 {
                    if triangles.len() >= MAX_TRIANGLES {
                        return Err(invalid("too many faces"));
                    }
                    let tri = [c[0], c[k], c[k + 1]];
                    let cols: Vec<Rgba8> = tri.iter().filter_map(|&i| colors[i]).collect();
                    let color = if cols.len() == 3 {
                        let avg = |f: fn(&Rgba8) -> u8| {
                            (cols.iter().map(|c| f(c) as u32).sum::<u32>() / 3) as u8
                        };
                        Rgba8::rgb(avg(|c| c.r), avg(|c| c.g), avg(|c| c.b))
                    } else {
                        color
                    };
                    triangles.push(Triangle {
                        p: tri.map(|i| positions[i]),
                        color,
                    });
                }
            }
        }
    }
    if triangles.is_empty() {
        return Err(invalid("the file contains no faces"));
    }
    Ok(triangles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_import::{project_from_mesh, UpAxis};
    use crate::{volume_from_runs, Pivot};
    use glam::IVec3;
    use rizxel_engine::VoxelizeOptions;
    use rizxel_math::{BoundSize, Palette};

    fn model() -> (VoxelVolume, BoundSize) {
        let bounds = BoundSize::new(8, 6, 5);
        let mut v = VoxelVolume::new(bounds);
        for x in 1..6 {
            v.set(IVec3::new(x, 0, 1), 9);
            v.set(IVec3::new(x, 1, 1), 40);
        }
        v.set(IVec3::new(3, 2, 1), 20);
        v.set(IVec3::new(3, 2, 2), 20);
        (v, bounds)
    }

    #[test]
    fn header_and_size_are_consistent() {
        let (v, b) = model();
        let ply = write_ply(&v, &Palette::default(), 0.1, Pivot::default().origin(b)).unwrap();
        let text = String::from_utf8_lossy(&ply);
        assert!(text.starts_with("ply\nformat binary_little_endian 1.0\n"));
        let quads = greedy_quads(&v).len();
        assert!(text.contains(&format!("element vertex {}\n", quads * 4)));
        let body = ply.windows(11).position(|w| w == b"end_header\n").unwrap() + 11;
        assert_eq!(ply.len() - body, quads * 4 * 27 + quads * 17);
    }

    #[test]
    fn export_imports_back_exactly_and_faces_outward() {
        let (v, b) = model();
        let palette = Palette::default();
        let ply = write_ply(&v, &palette, 0.1, Pivot::default().origin(b)).unwrap();
        let tris = read_ply(&ply, Rgba8::WHITE).unwrap();
        // Faces wind counter-clockwise: the face normal points away from
        // the model's center on every triangle of this convex-ish block.
        assert_eq!(tris.len(), greedy_quads(&v).len() * 2);
        let (p, info) = project_from_mesh(
            tris,
            UpAxis::Z,
            Some(palette.clone()),
            VoxelizeOptions::default(),
            "m",
        )
        .unwrap();
        assert!(info.exact);
        let back = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_eq!(back.voxel_count(), v.voxel_count());
        let mut got: Vec<(IVec3, u8)> = back.iter().collect();
        let mut want: Vec<(IVec3, u8)> = v
            .iter()
            .map(|(q, c)| (q - IVec3::new(1, 0, 1), c))
            .collect();
        got.sort_by_key(|(q, _)| (q.z, q.y, q.x));
        want.sort_by_key(|(q, _)| (q.z, q.y, q.x));
        assert_eq!(got, want);
    }

    #[test]
    fn reads_ascii_with_float_colors_and_extra_elements() {
        let text = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\n\
                    property float z\nproperty float red\nproperty float green\nproperty float blue\n\
                    element face 1\nproperty list uchar int vertex_indices\n\
                    element edge 1\nproperty int a\nproperty int b\nend_header\n\
                    0 0 0 1 0 0\n1 0 0 1 0 0\n1 1 0 1 0 0\n0 1 0 1 0 0\n4 0 1 2 3\n0 1\n";
        let t = read_ply(text.as_bytes(), Rgba8::WHITE).unwrap();
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|t| t.color == Rgba8::rgb(255, 0, 0)));
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let head = |fmt: &str, v: &str, f: &str| {
            format!(
                "ply\nformat {fmt} 1.0\nelement vertex {v}\nproperty float x\nproperty float y\n\
                 property float z\nelement face {f}\nproperty list uchar uint vertex_indices\nend_header\n"
            )
        };
        for bad in [
            b"".to_vec(),
            b"ply\nend_header\n".to_vec(),
            b"obj\nformat ascii 1.0\nend_header\n".to_vec(),
            head("ascii", "3", "1").into_bytes(),
            format!("{}0 0 0\n1 0 0\n0 1 0\n3 0 1 7\n", head("ascii", "3", "1")).into_bytes(),
            format!("{}0 0 0\n1 0 0\n0 1 0\n", head("ascii", "3", "0")).into_bytes(),
            head("binary_little_endian", "4000000000", "0").into_bytes(),
            head("binary_little_endian", "99", "0").into_bytes(),
            head("binary_mixed_endian", "1", "0").into_bytes(),
            format!(
                "{}0 0 0\n1 0 0\n0 1 0\n255 0 1 2\n",
                head("ascii", "3", "1")
            )
            .into_bytes(),
        ] {
            assert!(
                read_ply(&bad, Rgba8::WHITE).is_err(),
                "{:?}",
                String::from_utf8_lossy(&bad)
            );
        }
        assert!(write_ply(
            &VoxelVolume::new(BoundSize::new(2, 2, 2)),
            &Palette::default(),
            1.0,
            Vec3::ZERO
        )
        .is_err());
    }
}
