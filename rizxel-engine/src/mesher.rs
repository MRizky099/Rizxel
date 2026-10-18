//! Mesh data produced by the voxel mesher.
//!
//! Phase 2 only defines the data container (used by OBJ export). Greedy meshing
//! from voxel chunks is added in Phase 4.

use glam::Vec3;
use rizxel_math::Rgba8;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_cube_is_valid() {
        let m = Mesh::unit_cube(Rgba8::WHITE);
        assert!(m.is_valid());
        assert_eq!(m.vertex_count(), 8);
        assert_eq!(m.triangle_count(), 12);
    }
}
