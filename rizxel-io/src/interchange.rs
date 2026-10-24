//! Pieces shared by the import/export formats: coordinate frames, the
//! scene handed to exporters, and the builder that turns imported layers
//! into a validated [`ProjectFile`].
//!
//! ## Frames
//!
//! Rizxel is Y-up with voxels in `0..bounds`. MagicaVoxel, Goxel and STL are
//! Z-up. Converting is a proper rotation (−90° about X), so models are never
//! mirrored: Z-up `(x, y, z)` = Rizxel `(x, sz - 1 - z, y)` for voxel cells,
//! where `sz` is the Rizxel depth.

use glam::{IVec3, UVec3};
use rizxel_math::{
    BoundSize, LayerId, LayerKind, Material, Palette, VoxelVolume, FIRST_COLOR_INDEX,
};

use crate::{runs_from_volume, LayerData, ProjectFile, RzxlError};

/// Z-up model size for Rizxel `bounds`: `(sx, sz, sy)`.
pub fn zup_size(bounds: BoundSize) -> IVec3 {
    IVec3::new(bounds.x() as i32, bounds.z() as i32, bounds.y() as i32)
}

/// Rizxel voxel cell -> Z-up voxel cell (same model size).
pub fn to_zup(bounds: BoundSize, v: IVec3) -> IVec3 {
    IVec3::new(v.x, bounds.z() as i32 - 1 - v.z, v.y)
}

/// Z-up voxel cell -> Rizxel voxel cell, for a Z-up model of `size`.
pub fn from_zup(size: IVec3, p: IVec3) -> IVec3 {
    IVec3::new(p.x, p.z, size.y - 1 - p.y)
}

/// Z-up direction/offset -> Rizxel direction/offset.
pub fn dir_from_zup(d: IVec3) -> IVec3 {
    IVec3::new(d.x, d.z, -d.y)
}

/// Rizxel direction/offset -> Z-up direction/offset.
pub fn dir_to_zup(d: IVec3) -> IVec3 {
    IVec3::new(d.x, -d.z, d.y)
}

/// One layer of a scene being exported, with its evaluated geometry
/// (shapes rasterized, clones resolved, crop applied).
#[derive(Debug, Clone)]
pub struct ExportLayer {
    pub name: String,
    pub visible: bool,
    pub volume: VoxelVolume,
    /// Index into [`ExportScene::groups`] of the group holding the layer.
    pub group: Option<usize>,
    /// Surface material (`None` = the default material).
    pub material: Option<Material>,
    /// Layer opacity, `0..=1`.
    pub opacity: f32,
    /// For a linked clone whose geometry is exactly layer `.0`'s geometry
    /// moved by `.1` (no crops, nothing clipped at the bounds), so formats
    /// with instancing (glTF) can share one mesh.
    pub instance_of: Option<(usize, IVec3)>,
}

impl ExportLayer {
    /// A plain layer: no group, default material, opaque, not a clone.
    pub fn new(name: impl Into<String>, visible: bool, volume: VoxelVolume) -> Self {
        Self {
            name: name.into(),
            visible,
            volume,
            group: None,
            material: None,
            opacity: 1.0,
            instance_of: None,
        }
    }
}

/// A layer group of a scene being exported (`.vox` writes group nodes).
#[derive(Debug, Clone)]
pub struct ExportGroup {
    pub name: String,
    pub visible: bool,
    /// Index of the enclosing group in [`ExportScene::groups`].
    pub parent: Option<usize>,
    /// Number of [`ExportScene::layers`] drawn below the group: among its
    /// siblings the group sits between layer `order - 1` and layer `order`.
    pub order: usize,
}

/// What exporters that bake geometry receive.
#[derive(Debug, Clone)]
pub struct ExportScene {
    pub name: String,
    pub bounds: BoundSize,
    pub palette: Palette,
    /// Bottom to top.
    pub layers: Vec<ExportLayer>,
    /// Groups, bottom to top; exporters that bake geometry ignore them.
    pub groups: Vec<ExportGroup>,
}

impl ExportScene {
    /// Visible layers merged into one volume (later layers win).
    pub fn merged_visible(&self) -> VoxelVolume {
        let mut out = VoxelVolume::new(self.bounds);
        for l in self.layers.iter().filter(|l| l.visible) {
            for (p, v) in l.volume.iter() {
                out.set(p, v);
            }
        }
        out
    }
}

/// An imported layer before conversion to [`LayerData`].
#[derive(Debug, Clone)]
pub struct ImportLayer {
    /// Metadata; `voxels` is filled from `volume` by [`build_project`].
    pub data: LayerData,
    pub volume: Option<VoxelVolume>,
}

/// Smallest bounds holding `size`, or an error naming the limit.
pub fn bounds_for(size: UVec3, what: &str) -> Result<BoundSize, RzxlError> {
    if size.max_element() > BoundSize::MAX {
        return Err(RzxlError::Invalid(format!(
            "{what} is {}×{}×{} voxels; Rizxel supports up to {} per axis",
            size.x,
            size.y,
            size.z,
            BoundSize::MAX
        )));
    }
    Ok(BoundSize::new(size.x, size.y, size.z))
}

/// Builds and validates a project from imported pieces. Layer ids must
/// already be unique and non-zero; an empty list gets one empty layer.
pub fn build_project(
    name: impl Into<String>,
    bounds: BoundSize,
    palette: Palette,
    materials: Vec<Material>,
    layers: Vec<ImportLayer>,
) -> Result<ProjectFile, RzxlError> {
    let mut layers: Vec<LayerData> = layers
        .into_iter()
        .map(|l| {
            let mut data = l.data;
            if let Some(v) = &l.volume {
                data.voxels = runs_from_volume(bounds, v);
            }
            data
        })
        .collect();
    if layers.is_empty() {
        layers.push(LayerData::new(1, "Layer 1"));
    }
    let selected_color = (FIRST_COLOR_INDEX as usize).min(palette.len().saturating_sub(1));
    let project = ProjectFile {
        generator: crate::generator(),
        name: name.into(),
        palette,
        selected_color,
        materials,
        // Top layer selected, like after drawing a new one.
        active_layer: layers.len() - 1,
        layers,
        bounds,
        references: Vec::new(),
        render: Default::default(),
    };
    project.validate()?;
    Ok(project)
}

/// Final geometry of `project.layers[index]` (voxels, rasterized shape or
/// moved clone source, then crop), mirroring the document's evaluation.
/// `volumes[i]` holds the decoded runs of voxel layer `i`. `None` for groups
/// and broken clones.
pub fn evaluate_layer(
    project: &ProjectFile,
    volumes: &[Option<VoxelVolume>],
    index: usize,
) -> Option<VoxelVolume> {
    let layer = project.layers.get(index)?;
    let base = |i: usize| -> Option<VoxelVolume> {
        match project.layers.get(i)?.kind {
            LayerKind::Voxels => volumes.get(i)?.clone(),
            LayerKind::Shape { shape, area, color } => Some(rizxel_engine::rasterize_shape(
                shape,
                &area,
                color,
                project.bounds,
            )),
            _ => None,
        }
    };
    let geometry = match layer.kind {
        LayerKind::Group { .. } => return None,
        LayerKind::Clone { source, offset } => {
            let src = project.layers.iter().position(|l| l.id == source)?;
            base(src)?.translated(offset)
        }
        _ => base(index)?,
    };
    Some(match &layer.crop {
        Some(c) => geometry.cropped(c),
        None => geometry,
    })
}

/// Next free id after the ones in `layers`.
pub fn next_id(layers: &[ImportLayer]) -> LayerId {
    layers.iter().map(|l| l.data.id).max().unwrap_or(0) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zup_conversion_roundtrips_and_is_not_mirrored() {
        let b = BoundSize::new(4, 5, 6);
        let size = zup_size(b);
        assert_eq!(size, IVec3::new(4, 6, 5));
        for v in [IVec3::ZERO, IVec3::new(3, 4, 5), IVec3::new(1, 2, 3)] {
            let z = to_zup(b, v);
            assert!(z.cmpge(IVec3::ZERO).all() && z.cmplt(size).all());
            assert_eq!(from_zup(size, z), v);
        }
        // Rizxel up (+Y) is Z-up +Z; Rizxel +Z (toward the front) is -Y.
        assert_eq!(dir_to_zup(IVec3::Y), IVec3::Z);
        assert_eq!(dir_to_zup(IVec3::Z), IVec3::NEG_Y);
        assert_eq!(
            dir_from_zup(dir_to_zup(IVec3::new(1, 2, 3))),
            IVec3::new(1, 2, 3)
        );
        // Proper rotation: X × Y = Z holds in both frames.
        let (x, y) = (dir_to_zup(IVec3::X), dir_to_zup(IVec3::Y));
        assert_eq!(x.cross(y), dir_to_zup(IVec3::Z));
    }

    #[test]
    fn oversized_models_are_refused() {
        assert!(bounds_for(UVec3::new(256, 1, 1), "x").is_ok());
        let e = bounds_for(UVec3::new(257, 1, 1), "The model").unwrap_err();
        assert!(e.to_string().contains("up to 256"));
    }
}
