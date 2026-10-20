//! Turns imported triangle meshes (OBJ, STL) into a Rizxel project.
//!
//! - **Axes**: OBJ files are Y-up like Rizxel. STL files are Z-up and are
//!   rotated (not mirrored) into Y-up.
//! - **Voxelizing**: see [`rizxel_engine::voxelize`]. Voxel-editor exports
//!   import exactly at their own grid; other meshes are fit to the
//!   requested resolution.
//! - **Colors**: when the mesh carries a 256×1 palette texture (MagicaVoxel
//!   and Rizxel exports), that texture becomes the palette and colors keep
//!   their indices. Uncolored meshes (STL) use the default palette's
//!   selected color. Other colors are reduced to at most 255.

use std::collections::HashMap;

use glam::Vec3;
use rizxel_engine::{build_color_map, voxelize, Triangle, VoxelizeOptions};
use rizxel_math::{Palette, Rgba8, Voxel, VoxelVolume};

use crate::interchange::{bounds_for, build_project, ImportLayer};
use crate::{LayerData, ProjectFile, RzxlError};

/// Which axis points up in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpAxis {
    Y,
    Z,
}

/// Color used for meshes without colors (STL): the default palette's
/// highlighted swatch.
pub fn uncolored() -> Rgba8 {
    Palette::default()
        .get(Palette::DEFAULT_SELECTED)
        .unwrap_or(Rgba8::WHITE)
}

/// How a mesh was voxelized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshImportInfo {
    /// True when the mesh's own voxel grid was found (lossless import).
    pub exact: bool,
    /// Mesh units per voxel.
    pub voxel_size: f32,
}

/// Voxelizes `triangles` into a one-layer project named `name`.
pub fn project_from_mesh(
    mut triangles: Vec<Triangle>,
    up: UpAxis,
    palette_texture: Option<Palette>,
    options: VoxelizeOptions,
    name: &str,
) -> Result<(ProjectFile, MeshImportInfo), RzxlError> {
    if up == UpAxis::Z {
        for t in &mut triangles {
            t.p = t.p.map(|p| Vec3::new(p.x, p.z, -p.y));
        }
    }
    let grid = voxelize(&triangles, options).map_err(RzxlError::Invalid)?;
    let bounds = bounds_for(grid.size, "The voxelized mesh")?;

    let (palette, value): (Palette, Box<dyn Fn(Rgba8) -> Voxel>) = match palette_texture {
        Some(palette) => {
            // Exact colors keep their palette index (first match wins).
            let mut exact: HashMap<Rgba8, Voxel> = HashMap::new();
            for (i, c) in palette.colors().iter().enumerate().skip(1) {
                exact.entry(Rgba8::rgb(c.r, c.g, c.b)).or_insert(i as Voxel);
            }
            let fallback = palette.clone();
            (
                palette,
                Box::new(move |c: Rgba8| {
                    exact
                        .get(&c)
                        .copied()
                        .unwrap_or_else(|| rizxel_engine::nearest(&fallback, c))
                }),
            )
        }
        None => {
            let mut counts: Vec<(Rgba8, u64)> = Vec::new();
            let mut index: HashMap<Rgba8, usize> = HashMap::new();
            for (_, c) in &grid.cells {
                match index.get(c) {
                    Some(&i) => counts[i].1 += 1,
                    None => {
                        index.insert(*c, counts.len());
                        counts.push((*c, 1));
                    }
                }
            }
            let only = (counts.len() == 1).then(|| counts[0].0);
            if only == Some(uncolored()) {
                let palette = Palette::default();
                let selected = Palette::DEFAULT_SELECTED as Voxel;
                (palette, Box::new(move |_| selected))
            } else {
                let map = build_color_map(&counts);
                let palette = map.palette.clone();
                (palette, Box::new(move |c| map.value(c)))
            }
        }
    };

    let mut volume = VoxelVolume::new(bounds);
    for (p, c) in &grid.cells {
        volume.set(*p, value(*c));
    }
    let info = MeshImportInfo {
        exact: grid.exact,
        voxel_size: grid.voxel_size,
    };
    let project = build_project(
        name,
        bounds,
        palette,
        Vec::new(),
        vec![ImportLayer {
            data: LayerData::new(1, name),
            volume: Some(volume),
        }],
    )?;
    Ok((project, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_obj_export, read_obj, volume_from_runs};
    use glam::IVec3;
    use rizxel_math::BoundSize;

    #[test]
    fn rizxel_obj_export_imports_back_exactly() {
        let bounds = BoundSize::new(10, 8, 6);
        let mut v = VoxelVolume::new(bounds);
        for x in 2..7 {
            v.set(IVec3::new(x, 0, 2), 9);
            v.set(IVec3::new(x, 1, 2), 40);
        }
        v.set(IVec3::new(4, 2, 2), 200);
        v.set(IVec3::new(4, 2, 3), 200);
        let palette = Palette::default();
        let e = build_obj_export(&v, bounds, &palette, "m", 0.1).unwrap();
        let (mtl, png) = (e.mtl.clone(), e.png.clone());
        let load = move |n: &str| match n {
            "m.mtl" => Some(mtl.clone().into_bytes()),
            "m.png" => Some(png.clone()),
            _ => None,
        };
        let mesh = read_obj(&e.obj, &load).unwrap();
        let (p, info) = project_from_mesh(
            mesh.triangles,
            UpAxis::Y,
            mesh.palette,
            VoxelizeOptions::default(),
            "m",
        )
        .unwrap();
        assert!(info.exact);
        assert!((info.voxel_size - 0.1).abs() < 1e-5);
        // Bounds shrink to the occupied box; voxels and indices match.
        assert_eq!(p.bounds, BoundSize::new(5, 3, 2));
        let back = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        let shifted: Vec<(IVec3, u8)> = {
            let mut cells: Vec<_> = v
                .iter()
                .map(|(q, c)| (q - IVec3::new(2, 0, 2), c))
                .collect();
            cells.sort_by_key(|(q, _)| (q.z, q.y, q.x));
            cells
        };
        let mut got: Vec<_> = back.iter().collect();
        got.sort_by_key(|(q, _)| (q.z, q.y, q.x));
        assert_eq!(got, shifted);
    }

    #[test]
    fn stl_imports_upright_with_the_default_color() {
        let bounds = BoundSize::new(3, 5, 2);
        let mut v = VoxelVolume::new(bounds);
        for y in 0..5 {
            v.set(IVec3::new(1, y, 0), 4);
        }
        let stl = crate::format_stl::write_stl(&v, bounds, 1.0).unwrap();
        let tris = crate::format_stl::read_stl(&stl, uncolored()).unwrap();
        let (p, _) =
            project_from_mesh(tris, UpAxis::Z, None, VoxelizeOptions::default(), "s").unwrap();
        // A 5-tall column stays upright (Y), not lying along Z.
        assert_eq!(p.bounds, BoundSize::new(1, 5, 1));
        let back = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_eq!(back.voxel_count(), 5);
        assert!(back
            .iter()
            .all(|(_, c)| c as usize == Palette::DEFAULT_SELECTED));
    }
}
