//! Layer-level value types shared by the engine, the file format and the
//! document: layer kinds (voxels, groups, linked clones, shapes), crop boxes
//! and materials.

use glam::IVec3;
use serde::{Deserialize, Serialize};

use crate::types::{BoundSize, Voxel};

/// Stable, non-zero layer id, unique inside one document. Clones and group
/// membership refer to layers by id, so reordering layers never breaks links.
pub type LayerId = u32;

/// Integer box in voxel space: `min` inclusive, `max` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VoxelBox {
    pub min: IVec3,
    pub max: IVec3,
}

impl VoxelBox {
    /// Box spanning `a` and `b` (inclusive corners, any order).
    pub fn from_corners(a: IVec3, b: IVec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b) + IVec3::ONE,
        }
    }

    /// Box covering the whole model bounds.
    pub fn from_bounds(bounds: BoundSize) -> Self {
        Self {
            min: IVec3::ZERO,
            max: bounds.0.as_ivec3(),
        }
    }

    /// True when the box contains at least one voxel.
    pub fn is_valid(&self) -> bool {
        self.min.cmplt(self.max).all()
    }

    pub fn size(&self) -> IVec3 {
        (self.max - self.min).max(IVec3::ZERO)
    }

    pub fn contains(&self, v: IVec3) -> bool {
        v.cmpge(self.min).all() && v.cmplt(self.max).all()
    }

    /// Overlap of two boxes (may be invalid when they do not overlap).
    pub fn intersect(&self, other: &VoxelBox) -> VoxelBox {
        VoxelBox {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }
}

/// Primitive used by shape layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Cube,
    /// Ellipsoid inscribed in the shape box.
    Sphere,
    /// Elliptic cylinder along +Y inscribed in the shape box.
    Cylinder,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 3] = [ShapeKind::Cube, ShapeKind::Sphere, ShapeKind::Cylinder];

    pub const fn label(self) -> &'static str {
        match self {
            ShapeKind::Cube => "Cube",
            ShapeKind::Sphere => "Sphere",
            ShapeKind::Cylinder => "Cylinder",
        }
    }
}

/// What a layer contributes to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerKind {
    /// Hand-edited voxels stored in the layer itself.
    #[default]
    Voxels,
    /// Folder for other layers (children name it as `parent`). Has no
    /// geometry; hiding it hides its children.
    Group {
        #[serde(default)]
        collapsed: bool,
    },
    /// Linked clone: shows the voxels of layer `source`, moved by `offset`,
    /// and follows every change to the source. The source must be a voxel or
    /// shape layer.
    Clone {
        source: LayerId,
        #[serde(default)]
        offset: IVec3,
    },
    /// Procedural primitive filling `area` with palette index `color`.
    Shape {
        shape: ShapeKind,
        area: VoxelBox,
        color: Voxel,
    },
}

impl LayerKind {
    /// True for kinds that store their own voxels.
    pub fn stores_voxels(&self) -> bool {
        matches!(self, LayerKind::Voxels)
    }

    /// True for kinds a linked clone may point at.
    pub fn is_clone_source(&self) -> bool {
        matches!(self, LayerKind::Voxels | LayerKind::Shape { .. })
    }

    pub fn is_group(&self) -> bool {
        matches!(self, LayerKind::Group { .. })
    }
}

/// Surface material (PBR-style, like Goxel's). Colors still come from the
/// palette; `tint` multiplies them. Linear values in `0..=1`, except `ior`
/// ([`Material::IOR_RANGE`]) and `emission_strength`
/// ([`Material::MAX_EMISSION_STRENGTH`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    #[serde(default = "Material::default_metallic")]
    pub metallic: f32,
    #[serde(default = "Material::default_roughness")]
    pub roughness: f32,
    /// RGB emission added on top of the lit color.
    #[serde(default)]
    pub emission: [f32; 3],
    /// RGBA multiplier for the palette color (alpha = material opacity).
    #[serde(default = "Material::default_tint")]
    pub tint: [f32; 4],
    /// Strength of the dielectric reflection (0.5 = 4 % at normal
    /// incidence). Version 5+.
    #[serde(default = "Material::default_specular")]
    pub specular: f32,
    /// Index of refraction of transmissive materials (version 5+).
    #[serde(default = "Material::default_ior")]
    pub ior: f32,
    /// Share of light passing through (glass). Version 5+.
    #[serde(default)]
    pub transmission: f32,
    /// Clear glossy coat over the base (plastic). Version 5+.
    #[serde(default)]
    pub clearcoat: f32,
    /// Multiplier of `emission` in the path tracer (version 5+).
    #[serde(default = "Material::default_emission_strength")]
    pub emission_strength: f32,
}

impl Material {
    fn default_metallic() -> f32 {
        0.2
    }

    fn default_roughness() -> f32 {
        0.5
    }

    fn default_tint() -> [f32; 4] {
        [1.0; 4]
    }

    fn default_specular() -> f32 {
        0.5
    }

    fn default_ior() -> f32 {
        1.5
    }

    fn default_emission_strength() -> f32 {
        1.0
    }

    /// Accepted index of refraction.
    pub const IOR_RANGE: std::ops::RangeInclusive<f32> = 1.0..=3.0;
    /// Largest emission strength.
    pub const MAX_EMISSION_STRENGTH: f32 = 100.0;

    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metallic: Self::default_metallic(),
            roughness: Self::default_roughness(),
            emission: [0.0; 3],
            tint: Self::default_tint(),
            specular: Self::default_specular(),
            ior: Self::default_ior(),
            transmission: 0.0,
            clearcoat: 0.0,
            emission_strength: Self::default_emission_strength(),
        }
    }

    /// True when every channel is finite and inside its range.
    pub fn is_valid(&self) -> bool {
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        unit(self.metallic)
            && unit(self.roughness)
            && self.emission.iter().all(|&v| unit(v))
            && self.tint.iter().all(|&v| unit(v))
            && unit(self.specular)
            && unit(self.transmission)
            && unit(self.clearcoat)
            && self.ior.is_finite()
            && Self::IOR_RANGE.contains(&self.ior)
            && self.emission_strength.is_finite()
            && (0.0..=Self::MAX_EMISSION_STRENGTH).contains(&self.emission_strength)
    }

    /// Resets the surface parameters to `preset`'s, keeping the name and
    /// the tint color (opacity goes back to 1).
    pub fn apply_preset(&mut self, preset: MaterialPreset) {
        let [r, g, b, _] = self.tint;
        *self = Material {
            tint: [r, g, b, 1.0],
            ..Material::new(std::mem::take(&mut self.name))
        };
        match preset {
            MaterialPreset::Diffuse => {
                self.metallic = 0.0;
                self.roughness = 0.9;
                self.specular = 0.3;
            }
            MaterialPreset::Plastic => {
                self.metallic = 0.0;
                self.roughness = 0.35;
                self.clearcoat = 0.8;
            }
            MaterialPreset::Metal => {
                self.metallic = 1.0;
                self.roughness = 0.25;
            }
            MaterialPreset::Glass => {
                self.metallic = 0.0;
                self.roughness = 0.02;
                self.transmission = 1.0;
            }
            MaterialPreset::Emit => {
                self.metallic = 0.0;
                self.roughness = 0.8;
                self.emission = [1.0; 3];
                self.emission_strength = 4.0;
            }
        }
    }
}

/// Starting points for the full material parameter set (Render panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialPreset {
    Diffuse,
    Plastic,
    Metal,
    Glass,
    Emit,
}

impl MaterialPreset {
    pub const ALL: [MaterialPreset; 5] = [
        MaterialPreset::Diffuse,
        MaterialPreset::Plastic,
        MaterialPreset::Metal,
        MaterialPreset::Glass,
        MaterialPreset::Emit,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            MaterialPreset::Diffuse => "Diffuse",
            MaterialPreset::Plastic => "Plastic",
            MaterialPreset::Metal => "Metal",
            MaterialPreset::Glass => "Glass",
            MaterialPreset::Emit => "Emit",
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new("Default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_box_basics() {
        let b = VoxelBox::from_corners(IVec3::new(3, 0, 5), IVec3::new(1, 2, 1));
        assert_eq!(b.min, IVec3::new(1, 0, 1));
        assert_eq!(b.max, IVec3::new(4, 3, 6));
        assert_eq!(b.size(), IVec3::new(3, 3, 5));
        assert!(b.contains(IVec3::new(3, 2, 5)) && !b.contains(IVec3::new(4, 0, 1)));
        let none = b.intersect(&VoxelBox::from_corners(IVec3::splat(10), IVec3::splat(12)));
        assert!(!none.is_valid());
    }

    #[test]
    fn layer_kind_json_is_tagged() {
        let k = LayerKind::Clone {
            source: 4,
            offset: IVec3::new(1, 0, -2),
        };
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, r#"{"type":"clone","source":4,"offset":[1,0,-2]}"#);
        assert_eq!(serde_json::from_str::<LayerKind>(&json).unwrap(), k);
        assert_eq!(
            serde_json::from_str::<LayerKind>(r#"{"type":"group"}"#).unwrap(),
            LayerKind::Group { collapsed: false }
        );
    }

    #[test]
    fn material_defaults_and_validation() {
        let m: Material = serde_json::from_str(r#"{"name":"Glow"}"#).unwrap();
        assert_eq!(m, Material::new("Glow"));
        assert!(m.is_valid());
        let bad = Material {
            roughness: f32::NAN,
            ..Material::default()
        };
        assert!(!bad.is_valid());
        let bad = Material {
            ior: 0.5,
            ..Material::default()
        };
        assert!(!bad.is_valid());
    }

    #[test]
    fn material_presets_keep_name_and_color() {
        let mut m = Material {
            tint: [0.5, 0.2, 0.1, 0.4],
            ..Material::new("Brick")
        };
        m.apply_preset(MaterialPreset::Glass);
        assert_eq!(m.name, "Brick");
        assert_eq!(m.tint, [0.5, 0.2, 0.1, 1.0]);
        assert_eq!(m.transmission, 1.0);
        for p in MaterialPreset::ALL {
            m.apply_preset(p);
            assert!(m.is_valid(), "{p:?}");
        }
        // Version 3/4 bodies have none of the new fields.
        let old: Material =
            serde_json::from_str(r#"{"name":"Old","metallic":0.1,"roughness":0.4}"#).unwrap();
        assert_eq!(
            (
                old.specular,
                old.ior,
                old.transmission,
                old.emission_strength
            ),
            (0.5, 1.5, 0.0, 1.0)
        );
    }
}
