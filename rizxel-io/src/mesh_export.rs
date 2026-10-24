//! Settings shared by the mesh exports (OBJ, glTF, STL, PLY).

use glam::Vec3;
use rizxel_math::BoundSize;

/// Where the model's origin goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pivot {
    /// Centered on X and Z with integer halves, standing on Y = 0: the
    /// MagicaVoxel OBJ placement, so imports from MagicaVoxel export to the
    /// same coordinates.
    #[default]
    BottomCenter,
    /// The exact center of the bounds.
    Center,
    /// The bounds' minimum corner (all coordinates positive).
    Corner,
}

impl Pivot {
    pub const ALL: [Pivot; 3] = [Pivot::BottomCenter, Pivot::Center, Pivot::Corner];

    pub const fn label(self) -> &'static str {
        match self {
            Pivot::BottomCenter => "Bottom center",
            Pivot::Center => "Center",
            Pivot::Corner => "Corner",
        }
    }

    /// The Rizxel (Y-up) voxel-corner point that becomes the origin.
    pub fn origin(self, bounds: BoundSize) -> Vec3 {
        let (x, y, z) = (bounds.x(), bounds.y(), bounds.z());
        match self {
            Pivot::BottomCenter => Vec3::new((x / 2) as f32, 0.0, z.div_ceil(2) as f32),
            Pivot::Center => Vec3::new(x as f32, y as f32, z as f32) / 2.0,
            Pivot::Corner => Vec3::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let b = BoundSize::new(5, 4, 7);
        assert_eq!(Pivot::BottomCenter.origin(b), Vec3::new(2.0, 0.0, 4.0));
        assert_eq!(Pivot::Center.origin(b), Vec3::new(2.5, 2.0, 3.5));
        assert_eq!(Pivot::Corner.origin(b), Vec3::ZERO);
    }
}
