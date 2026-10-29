//! Tiled, sparse voxel storage (Goxel-style).
//!
//! The volume is split into 16³ tiles kept in a hash map keyed by the tile's
//! origin in world voxel space. Tiles that contain no voxels are never stored,
//! so an empty or sparse model costs almost nothing and growing the bounds is
//! O(existing tiles) instead of O(new volume).
//!
//! Voxels outside the volume's [`BoundSize`] (`0..size` on every axis) read as
//! empty and writes to them are ignored.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use glam::{IVec3, Vec3};

use crate::types::{BoundSize, Voxel, EMPTY_VOXEL};
use crate::{Aabb, VoxelBox};

/// Edge length of a tile, in voxels.
pub const TILE_SIZE: i32 = 16;
/// Number of voxels in one tile.
pub const TILE_VOLUME: usize = (TILE_SIZE * TILE_SIZE * TILE_SIZE) as usize;

/// Tile coordinate (`voxel.div_euclid(TILE_SIZE)`), valid for negative voxels too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TilePos(pub IVec3);

impl TilePos {
    /// Tile that contains the voxel `v`.
    pub fn from_voxel(v: IVec3) -> Self {
        Self(v.div_euclid(IVec3::splat(TILE_SIZE)))
    }

    /// World voxel coordinate of the tile's minimum corner (a multiple of 16).
    pub fn origin(self) -> IVec3 {
        self.0 * TILE_SIZE
    }

    /// Index of `v` inside this tile, or `None` when `v` lies in another tile.
    pub fn local(self, v: IVec3) -> Option<usize> {
        let l = v - self.origin();
        let inside = l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(TILE_SIZE)).all();
        inside.then(|| local_index(l))
    }
}

/// Linear index of a tile-local coordinate (`x` fastest, then `y`, then `z`).
#[inline]
pub fn local_index(l: IVec3) -> usize {
    (l.x + l.y * TILE_SIZE + l.z * TILE_SIZE * TILE_SIZE) as usize
}

/// Tile-local coordinate of a linear index (inverse of [`local_index`]).
#[inline]
pub fn local_coord(i: usize) -> IVec3 {
    let i = i as i32;
    IVec3::new(
        i % TILE_SIZE,
        (i / TILE_SIZE) % TILE_SIZE,
        i / (TILE_SIZE * TILE_SIZE),
    )
}

/// 16³ block of voxels. Copy-on-write: cloning a volume shares its tiles
/// until one is written, so snapshots (undo, the mesh cache, autosave)
/// stay cheap and unchanged tiles compare by pointer.
#[derive(Debug, Clone, Eq)]
pub struct Tile {
    voxels: Arc<[Voxel; TILE_VOLUME]>,
    /// Number of non-empty voxels, so emptiness checks are O(1).
    count: u16,
}

impl Tile {
    fn new() -> Self {
        Self {
            voxels: Arc::new([EMPTY_VOXEL; TILE_VOLUME]),
            count: 0,
        }
    }

    /// Shared all-empty tile returned for unallocated reads.
    pub fn empty() -> &'static Tile {
        static EMPTY: OnceLock<Tile> = OnceLock::new();
        EMPTY.get_or_init(Tile::new)
    }

    /// Voxel at a linear local index (empty when out of range).
    #[inline]
    pub fn get(&self, index: usize) -> Voxel {
        self.voxels.get(index).copied().unwrap_or(EMPTY_VOXEL)
    }

    /// Voxel at a tile-local coordinate in `0..16` (empty when out of range).
    #[inline]
    pub fn get_local(&self, l: IVec3) -> Voxel {
        if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(TILE_SIZE)).all() {
            self.get(local_index(l))
        } else {
            EMPTY_VOXEL
        }
    }

    /// Writes a voxel and returns the previous value.
    fn set(&mut self, index: usize, v: Voxel) -> Voxel {
        let old = match self.voxels.get(index) {
            Some(&old) if old == v => return old,
            Some(&old) => old,
            None => return EMPTY_VOXEL,
        };
        // Copies the array only while another volume shares it.
        Arc::make_mut(&mut self.voxels)[index] = v;
        match (old == EMPTY_VOXEL, v == EMPTY_VOXEL) {
            (true, false) => self.count += 1,
            (false, true) => self.count -= 1,
            _ => {}
        }
        old
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Number of non-empty voxels in this tile.
    pub fn count(&self) -> usize {
        self.count as usize
    }

    /// Raw voxel array (`x` fastest, see [`local_index`]).
    pub fn voxels(&self) -> &[Voxel; TILE_VOLUME] {
        &self.voxels
    }
}

impl PartialEq for Tile {
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count
            && (Arc::ptr_eq(&self.voxels, &other.voxels) || self.voxels == other.voxels)
    }
}

/// Sparse voxel grid limited to `0..bounds` on every axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxelVolume {
    bounds: BoundSize,
    /// Keyed by tile origin (multiple of [`TILE_SIZE`]).
    tiles: HashMap<IVec3, Tile>,
    non_empty: usize,
}

impl VoxelVolume {
    pub fn new(bounds: BoundSize) -> Self {
        Self {
            bounds,
            tiles: HashMap::new(),
            non_empty: 0,
        }
    }

    pub fn bounds(&self) -> BoundSize {
        self.bounds
    }

    /// True when `v` lies inside `0..bounds`.
    pub fn in_bounds(&self, v: IVec3) -> bool {
        v.cmpge(IVec3::ZERO).all() && v.as_uvec3().cmplt(self.bounds.0).all()
    }

    pub fn get(&self, v: IVec3) -> Voxel {
        if !self.in_bounds(v) {
            return EMPTY_VOXEL;
        }
        let tile = TilePos::from_voxel(v);
        match (self.tiles.get(&tile.origin()), tile.local(v)) {
            (Some(t), Some(i)) => t.get(i),
            _ => EMPTY_VOXEL,
        }
    }

    /// Writes a voxel and returns the previous value. Out-of-bounds writes are
    /// ignored (and return [`EMPTY_VOXEL`]).
    pub fn set(&mut self, v: IVec3, value: Voxel) -> Voxel {
        if !self.in_bounds(v) {
            return EMPTY_VOXEL;
        }
        let tile = TilePos::from_voxel(v);
        let Some(index) = tile.local(v) else {
            return EMPTY_VOXEL;
        };
        let key = tile.origin();
        if value == EMPTY_VOXEL {
            let Some(t) = self.tiles.get_mut(&key) else {
                return EMPTY_VOXEL;
            };
            let old = t.set(index, value);
            if old != EMPTY_VOXEL {
                self.non_empty -= 1;
            }
            if t.is_empty() {
                self.tiles.remove(&key);
            }
            old
        } else {
            let old = self
                .tiles
                .entry(key)
                .or_insert_with(Tile::new)
                .set(index, value);
            if old == EMPTY_VOXEL {
                self.non_empty += 1;
            }
            old
        }
    }

    /// Clears one voxel; returns the previous value.
    pub fn clear_at(&mut self, v: IVec3) -> Voxel {
        self.set(v, EMPTY_VOXEL)
    }

    /// Removes every voxel (bounds are kept).
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.non_empty = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.non_empty == 0
    }

    /// Number of non-empty voxels.
    pub fn voxel_count(&self) -> usize {
        self.non_empty
    }

    /// Number of allocated (non-empty) tiles.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Tile at `origin`, or the shared empty tile when none is allocated.
    pub fn tile(&self, origin: IVec3) -> &Tile {
        self.tiles.get(&origin).unwrap_or_else(|| Tile::empty())
    }

    /// Smallest box holding every voxel: `(min inclusive, max exclusive)`.
    pub fn occupied_bounds(&self) -> Option<(IVec3, IVec3)> {
        let mut acc: Option<(IVec3, IVec3)> = None;
        for (pos, v) in self.iter() {
            debug_assert!(v != EMPTY_VOXEL);
            let (lo, hi) = acc.get_or_insert((pos, pos + IVec3::ONE));
            *lo = lo.min(pos);
            *hi = hi.max(pos + IVec3::ONE);
        }
        acc
    }

    /// Allocated tiles (origin, tile), optionally only those overlapping `area`.
    pub fn tiles_in<'a>(
        &'a self,
        area: Option<&'a Aabb>,
    ) -> impl Iterator<Item = (&'a IVec3, &'a Tile)> + 'a {
        self.tiles.iter().filter(move |(origin, _)| {
            area.is_none_or(|a| {
                let lo = origin.as_vec3();
                let hi = lo + Vec3::splat(TILE_SIZE as f32);
                lo.cmplt(a.max).all() && hi.cmpgt(a.min).all()
            })
        })
    }

    /// Tile origins in a stable (sorted) order, for deterministic output.
    pub fn sorted_tile_origins(&self) -> Vec<IVec3> {
        let mut keys: Vec<IVec3> = self.tiles.keys().copied().collect();
        keys.sort_by_key(|k| (k.z, k.y, k.x));
        keys
    }

    /// Every non-empty voxel as `(world position, value)` in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = (IVec3, Voxel)> + '_ {
        self.tiles.iter().flat_map(|(origin, tile)| {
            tile.voxels
                .iter()
                .enumerate()
                .filter(|(_, &v)| v != EMPTY_VOXEL)
                .map(move |(i, &v)| (*origin + local_coord(i), v))
        })
    }

    /// Changes the bounds, dropping voxels that fall outside. Cost is
    /// proportional to the allocated tiles, not to the new volume.
    pub fn resize(&mut self, bounds: BoundSize) {
        if bounds == self.bounds {
            return;
        }
        self.bounds = bounds;
        let limit = bounds.0.as_ivec3();
        let mut removed = 0usize;
        self.tiles.retain(|origin, tile| {
            let end = *origin + IVec3::splat(TILE_SIZE);
            if end.cmple(limit).all() {
                return true; // fully inside
            }
            if origin.cmpge(limit).any() {
                removed += tile.count();
                return false; // fully outside
            }
            for i in 0..TILE_VOLUME {
                let p = *origin + local_coord(i);
                if p.cmpge(limit).any() && tile.set(i, EMPTY_VOXEL) != EMPTY_VOXEL {
                    removed += 1;
                }
            }
            !tile.is_empty()
        });
        self.non_empty -= removed;
    }

    /// Copy moved by `offset`; voxels that leave the bounds are dropped.
    pub fn translated(&self, offset: IVec3) -> VoxelVolume {
        if offset == IVec3::ZERO {
            return self.clone();
        }
        let mut out = VoxelVolume::new(self.bounds);
        for (pos, v) in self.iter() {
            out.set(pos + offset, v);
        }
        out
    }

    /// Copy keeping only the voxels inside `area`.
    pub fn cropped(&self, area: &VoxelBox) -> VoxelVolume {
        let mut out = self.clone();
        let outside: Vec<IVec3> = self
            .iter()
            .map(|(p, _)| p)
            .filter(|p| !area.contains(*p))
            .collect();
        for p in outside {
            out.clear_at(p);
        }
        out
    }
}

impl Default for VoxelVolume {
    fn default() -> Self {
        Self::new(BoundSize::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(n: u32) -> VoxelVolume {
        VoxelVolume::new(BoundSize::new(n, n, n))
    }

    #[test]
    fn roundtrip_across_tile_boundaries() {
        let mut v = vol(64);
        for axis in [IVec3::X, IVec3::Y, IVec3::Z] {
            let a = axis * 15;
            let b = axis * 16;
            v.set(a, 3);
            v.set(b, 4);
            assert_eq!(v.get(a), 3);
            assert_eq!(v.get(b), 4);
            assert_ne!(TilePos::from_voxel(a), TilePos::from_voxel(b));
        }
        // The three `15` voxels share tile 0; each `16` voxel opens its own.
        assert_eq!(v.voxel_count(), 6);
        assert_eq!(v.tile_count(), 4);
    }

    #[test]
    fn clearing_a_tile_drops_it() {
        let mut v = vol(32);
        let cells: Vec<IVec3> = (0..TILE_VOLUME).map(local_coord).collect();
        for &c in &cells {
            v.set(c, 1);
        }
        assert_eq!(v.voxel_count(), TILE_VOLUME);
        assert_eq!(v.tile_count(), 1);
        for &c in &cells {
            assert_eq!(v.clear_at(c), 1);
        }
        assert_eq!(v.tile_count(), 0);
        assert_eq!(v.voxel_count(), 0);
        assert!(v.is_empty());
    }

    #[test]
    fn overwrite_keeps_count() {
        let mut v = vol(8);
        assert_eq!(v.set(IVec3::ONE, 5), 0);
        assert_eq!(v.set(IVec3::ONE, 6), 5);
        assert_eq!(v.voxel_count(), 1);
        assert_eq!(v.clear_at(IVec3::new(2, 2, 2)), 0);
        assert_eq!(v.voxel_count(), 1);
    }

    #[test]
    fn occupied_bounds_of_scattered_voxels() {
        let mut v = vol(64);
        assert_eq!(v.occupied_bounds(), None);
        v.set(IVec3::new(3, 40, 7), 1);
        v.set(IVec3::new(20, 2, 9), 2);
        v.set(IVec3::new(5, 5, 33), 3);
        assert_eq!(
            v.occupied_bounds(),
            Some((IVec3::new(3, 2, 7), IVec3::new(21, 41, 34)))
        );
    }

    #[test]
    fn resize_shrink_then_grow() {
        let mut v = vol(40);
        v.set(IVec3::new(1, 1, 1), 1);
        v.set(IVec3::new(17, 2, 3), 2); // second tile, survives 20
        v.set(IVec3::new(25, 0, 0), 3); // dropped by 20
        v.set(IVec3::new(35, 35, 35), 4); // tile fully outside 20
        v.resize(BoundSize::new(20, 20, 20));
        assert_eq!(v.voxel_count(), 2);
        assert_eq!(v.get(IVec3::new(17, 2, 3)), 2);
        assert_eq!(v.get(IVec3::new(25, 0, 0)), 0);

        v.resize(BoundSize::new(64, 64, 64));
        assert_eq!(v.voxel_count(), 2);
        assert_eq!(v.get(IVec3::new(1, 1, 1)), 1);
        assert_eq!(v.get(IVec3::new(25, 0, 0)), 0, "dropped voxels stay gone");

        // Out-of-bounds writes are ignored.
        let mut small = vol(4);
        assert_eq!(small.set(IVec3::new(4, 0, 0), 9), 0);
        assert_eq!(small.set(IVec3::new(-1, 0, 0), 9), 0);
        assert!(small.is_empty());
    }

    #[test]
    fn negative_coordinates_map_to_correct_tile() {
        let t = TilePos::from_voxel(IVec3::new(-1, 0, 0));
        assert_eq!(t.0, IVec3::new(-1, 0, 0));
        assert_eq!(t.origin(), IVec3::new(-16, 0, 0));
        assert_eq!(t.local(IVec3::new(-1, 0, 0)), Some(15));

        let t = TilePos::from_voxel(IVec3::new(-17, -16, 15));
        assert_eq!(t.0, IVec3::new(-2, -1, 0));
        assert_eq!(t.origin(), IVec3::new(-32, -16, 0));
        assert_eq!(
            t.local(IVec3::new(-17, -16, 15)),
            Some(local_index(IVec3::new(15, 0, 15)))
        );
        assert_eq!(t.local(IVec3::new(0, 0, 0)), None);
    }

    #[test]
    fn misses_read_the_shared_empty_tile() {
        let v = vol(32);
        assert!(std::ptr::eq(v.tile(IVec3::ZERO), Tile::empty()));
        assert_eq!(v.get(IVec3::new(5, 5, 5)), 0);
    }

    #[test]
    fn local_index_roundtrip() {
        for i in [0, 1, 15, 16, 255, 256, 4095] {
            assert_eq!(local_index(local_coord(i)), i);
        }
    }

    #[test]
    fn translate_and_crop() {
        let mut v = vol(8);
        v.set(IVec3::new(0, 0, 0), 1);
        v.set(IVec3::new(7, 0, 0), 2);
        let moved = v.translated(IVec3::new(1, 2, 0));
        assert_eq!(moved.voxel_count(), 1, "x = 8 leaves the bounds");
        assert_eq!(moved.get(IVec3::new(1, 2, 0)), 1);

        let area = VoxelBox::from_corners(IVec3::ZERO, IVec3::new(3, 3, 3));
        let cropped = v.cropped(&area);
        assert_eq!(cropped.voxel_count(), 1);
        assert_eq!(cropped.get(IVec3::ZERO), 1);
        assert_eq!(v.voxel_count(), 2, "source untouched");
    }

    #[test]
    fn tiles_in_filters_by_area() {
        let mut v = vol(64);
        v.set(IVec3::new(1, 1, 1), 1);
        v.set(IVec3::new(40, 1, 1), 1);
        let area = Aabb::new(Vec3::ZERO, Vec3::splat(10.0));
        assert_eq!(v.tiles_in(Some(&area)).count(), 1);
        assert_eq!(v.tiles_in(None).count(), 2);
    }
}
