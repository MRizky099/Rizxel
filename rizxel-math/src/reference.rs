//! Reference images: concept art placed on an axis plane of the workspace
//! for tracing. The image file's own bytes (PNG or JPEG) are kept, so a
//! project carries its references with it.

use std::fmt;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::BoundSize;

/// Stable, non-zero reference id, unique inside one document.
pub type ReferenceId = u32;

/// Workspace plane a reference image lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefPlane {
    /// Front / back plane (normal Z), read from the Front view.
    #[default]
    Xy,
    /// Floor plane (normal Y), read from the Top view.
    Xz,
    /// Side plane (normal X), read from the Right view.
    Yz,
}

impl RefPlane {
    pub const ALL: [RefPlane; 3] = [RefPlane::Xz, RefPlane::Xy, RefPlane::Yz];

    pub const fn label(self) -> &'static str {
        match self {
            RefPlane::Xy => "XY",
            RefPlane::Xz => "XZ",
            RefPlane::Yz => "YZ",
        }
    }

    /// Axis along the plane's normal (0 = X, 1 = Y, 2 = Z).
    pub const fn normal_axis(self) -> usize {
        match self {
            RefPlane::Xy => 2,
            RefPlane::Xz => 1,
            RefPlane::Yz => 0,
        }
    }

    /// Image right and up directions, as seen from the plane's view (Front,
    /// Top or Right), so the image is never mirrored there.
    pub fn basis(self) -> (Vec3, Vec3) {
        match self {
            RefPlane::Xy => (Vec3::X, Vec3::Y),
            RefPlane::Xz => (Vec3::X, Vec3::NEG_Z),
            RefPlane::Yz => (Vec3::NEG_Z, Vec3::Y),
        }
    }

    /// Axes of the image's right and up directions.
    pub fn plane_axes(self) -> (usize, usize) {
        let (r, u) = self.basis();
        (axis_of(r), axis_of(u))
    }
}

fn axis_of(v: Vec3) -> usize {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z {
        0
    } else if a.y >= a.z {
        1
    } else {
        2
    }
}

/// How a reference is drawn against the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthMode {
    /// Depth-tested against the model: the usual tracing setup.
    #[default]
    Behind,
    /// Drawn over the model at its opacity.
    OnTop,
}

impl DepthMode {
    pub const ALL: [DepthMode; 2] = [DepthMode::Behind, DepthMode::OnTop];

    pub const fn label(self) -> &'static str {
        match self {
            DepthMode::Behind => "Behind model",
            DepthMode::OnTop => "On top",
        }
    }
}

/// When a reference shows, relative to Slice Mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceFilter {
    #[default]
    Always,
    /// Hidden while slicing.
    HiddenInSlice,
    /// Shown only while slicing.
    SliceOnly,
}

impl SliceFilter {
    pub const ALL: [SliceFilter; 3] = [
        SliceFilter::Always,
        SliceFilter::HiddenInSlice,
        SliceFilter::SliceOnly,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SliceFilter::Always => "Always",
            SliceFilter::HiddenInSlice => "Hidden in Slice Mode",
            SliceFilter::SliceOnly => "Slice Mode only",
        }
    }

    pub const fn shows(self, slicing: bool) -> bool {
        match self {
            SliceFilter::Always => true,
            SliceFilter::HiddenInSlice => !slicing,
            SliceFilter::SliceOnly => slicing,
        }
    }
}

/// An image file's bytes (PNG or JPEG) and its pixel size. Shared, so undo
/// steps that change a reference's placement do not copy the image.
/// Serialized as `{"width", "height", "data"}` with `data` in base64.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageData {
    bytes: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}

impl ImageData {
    /// Wraps encoded image bytes. The size is not checked here; the file
    /// layer decodes the image to validate it.
    pub fn new(bytes: impl Into<Arc<[u8]>>, width: u32, height: u32) -> Self {
        Self {
            bytes: bytes.into(),
            width,
            height,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Process-local identity of the bytes (texture caches key on it).
    pub fn key(&self) -> usize {
        self.bytes.as_ptr() as usize
    }
}

impl fmt::Debug for ImageData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ImageData({}×{}, {} bytes)",
            self.width,
            self.height,
            self.bytes.len()
        )
    }
}

#[derive(Serialize, Deserialize)]
struct ImageDataFile {
    width: u32,
    height: u32,
    data: String,
}

impl Serialize for ImageData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ImageDataFile {
            width: self.width,
            height: self.height,
            data: crate::base64::encode(&self.bytes),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for ImageData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let file = ImageDataFile::deserialize(d)?;
        let bytes = crate::base64::decode(&file.data)
            .map_err(|e| serde::de::Error::custom(format!("image data: {e}")))?;
        Ok(ImageData::new(bytes, file.width, file.height))
    }
}

fn swap(v: Vec2) -> Vec2 {
    Vec2::new(v.y, v.x)
}

fn default_opacity() -> f32 {
    0.5
}

fn default_true() -> bool {
    true
}

/// A reference image placed in the workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceImage {
    pub id: ReferenceId,
    pub name: String,
    pub image: ImageData,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Locked references keep their placement (the panel disables it).
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub plane: RefPlane,
    /// Voxels from the bounds' min corner. The component along the plane's
    /// normal is the plane position; the other two are the low corner of
    /// the image rectangle on that plane.
    #[serde(default)]
    pub offset: Vec3,
    /// Image width and height in voxels (before rotation).
    pub size: Vec2,
    /// Quarter turns, counterclockwise as seen from the plane's view.
    #[serde(default)]
    pub rotation: u8,
    #[serde(default)]
    pub flip_u: bool,
    #[serde(default)]
    pub flip_v: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub depth: DepthMode,
    #[serde(default)]
    pub slice: SliceFilter,
}

impl ReferenceImage {
    /// New reference on `plane`, fitted into the bounds face (the image's
    /// aspect ratio kept), centered on it and at the face's low end along
    /// the normal: behind the model as seen from the plane's view.
    pub fn fitted(
        id: ReferenceId,
        name: impl Into<String>,
        image: ImageData,
        plane: RefPlane,
        bounds: BoundSize,
    ) -> Self {
        let mut r = Self {
            id,
            name: name.into(),
            image,
            visible: true,
            locked: false,
            plane,
            offset: Vec3::ZERO,
            size: Vec2::ONE,
            rotation: 0,
            flip_u: false,
            flip_v: false,
            opacity: default_opacity(),
            depth: DepthMode::default(),
            slice: SliceFilter::default(),
        };
        r.fit_to(bounds);
        r
    }

    /// Fits the size into the bounds face and centers the image on it,
    /// keeping the position along the normal (*Reset Size*).
    pub fn fit_to(&mut self, bounds: BoundSize) {
        let (a, b) = self.plane.plane_axes();
        let face = Vec2::new(bounds.0[a] as f32, bounds.0[b] as f32);
        let px = Vec2::new(
            self.image.width.max(1) as f32,
            self.image.height.max(1) as f32,
        );
        // Rotated images fill the face the other way round.
        let px_on_face = if self.rotation % 2 == 1 { swap(px) } else { px };
        let scale = (face / px_on_face).min_element();
        self.size = px * scale;
        let extent = self.extent();
        self.offset[a] = (face.x - extent.x) * 0.5;
        self.offset[b] = (face.y - extent.y) * 0.5;
    }

    /// Size of the image rectangle along the plane's right and up axes
    /// (width and height swap for odd quarter turns).
    pub fn extent(&self) -> Vec2 {
        if self.rotation % 2 == 1 {
            swap(self.size)
        } else {
            self.size
        }
    }

    /// Moves the plane to the min (0), center (1) or max (2) of the bounds
    /// along its normal.
    pub fn snap_to_face(&mut self, bounds: BoundSize, which: u8) {
        let n = self.plane.normal_axis();
        let len = bounds.0[n] as f32;
        self.offset[n] = match which {
            0 => 0.0,
            1 => len * 0.5,
            _ => len,
        };
    }

    /// Corners in voxel space (relative to the bounds' min corner) for the
    /// image's top-left, top-right, bottom-right and bottom-left pixels,
    /// after rotation and flips.
    pub fn corners(&self) -> [Vec3; 4] {
        let (right, up) = self.plane.basis();
        let (a, b) = self.plane.plane_axes();
        let n = self.plane.normal_axis();
        let extent = self.extent();
        let mut center = Vec3::ZERO;
        center[a] = self.offset[a] + extent.x * 0.5;
        center[b] = self.offset[b] + extent.y * 0.5;
        center[n] = self.offset[n];
        let (hw, hh) = (self.size.x * 0.5, self.size.y * 0.5);
        [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].map(|(sx, sy)| {
            let mut x = sx * hw;
            let mut y = sy * hh;
            if self.flip_u {
                x = -x;
            }
            if self.flip_v {
                y = -y;
            }
            for _ in 0..self.rotation % 4 {
                (x, y) = (-y, x);
            }
            center + right * x + up * y
        })
    }

    /// True when every number is finite and in a sane range (the file
    /// layer adds size caps).
    pub fn is_valid(&self) -> bool {
        let finite = self.offset.is_finite() && self.size.is_finite();
        finite
            && self.size.cmpgt(Vec2::ZERO).all()
            && self.rotation < 4
            && self.opacity.is_finite()
            && (0.0..=1.0).contains(&self.opacity)
    }

    /// True when the offset stays within `limit` voxels of the origin and
    /// the size within `limit`.
    pub fn within(&self, limit: f32) -> bool {
        self.offset.abs().cmple(Vec3::splat(limit)).all()
            && self.size.cmple(Vec2::splat(limit)).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32) -> ImageData {
        ImageData::new(vec![1, 2, 3], w, h)
    }

    #[test]
    fn fitted_xy_image_covers_the_z0_face() {
        // 200×100 image on a 20×10×8 model: fills the XY face exactly.
        let bounds = BoundSize::new(20, 10, 8);
        let r = ReferenceImage::fitted(1, "front", image(200, 100), RefPlane::Xy, bounds);
        assert_eq!(r.size, Vec2::new(20.0, 10.0));
        assert_eq!(r.offset, Vec3::ZERO);
        let [tl, tr, br, bl] = r.corners();
        assert_eq!(tl, Vec3::new(0.0, 10.0, 0.0));
        assert_eq!(tr, Vec3::new(20.0, 10.0, 0.0));
        assert_eq!(br, Vec3::new(20.0, 0.0, 0.0));
        assert_eq!(bl, Vec3::ZERO);
    }

    #[test]
    fn fit_keeps_aspect_and_centers() {
        let bounds = BoundSize::new(32, 32, 32);
        // Tall image on the floor: height fills Z, centered along X.
        let r = ReferenceImage::fitted(1, "top", image(100, 200), RefPlane::Xz, bounds);
        assert_eq!(r.size, Vec2::new(16.0, 32.0));
        assert_eq!(r.offset, Vec3::new(8.0, 0.0, 0.0));
        // Seen from the top, image up is -Z: the top edge is at z = 0.
        let [tl, tr, _, bl] = r.corners();
        assert_eq!(tl, Vec3::new(8.0, 0.0, 0.0));
        assert_eq!(tr, Vec3::new(24.0, 0.0, 0.0));
        assert_eq!(bl, Vec3::new(8.0, 0.0, 32.0));
    }

    #[test]
    fn side_plane_reads_from_the_right() {
        let bounds = BoundSize::new(8, 16, 16);
        let mut r = ReferenceImage::fitted(1, "side", image(10, 10), RefPlane::Yz, bounds);
        r.snap_to_face(bounds, 2);
        assert_eq!(r.offset.x, 8.0);
        // From +X, image right is -Z: the left edge is at z = 16.
        let [tl, tr, ..] = r.corners();
        assert_eq!(tl, Vec3::new(8.0, 16.0, 16.0));
        assert_eq!(tr, Vec3::new(8.0, 16.0, 0.0));
    }

    #[test]
    fn rotation_and_flips_move_corners() {
        let bounds = BoundSize::new(20, 10, 8);
        let mut r = ReferenceImage::fitted(1, "f", image(200, 100), RefPlane::Xy, bounds);
        r.flip_u = true;
        let [tl, tr, ..] = r.corners();
        assert_eq!(
            (tl, tr),
            (Vec3::new(20.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0))
        );
        r.flip_u = false;
        // A quarter turn swaps the extent and keeps the rectangle's corner.
        r.rotation = 1;
        r.fit_to(bounds);
        assert_eq!(r.extent(), Vec2::new(5.0, 10.0));
        assert_eq!(r.size, Vec2::new(10.0, 5.0));
        // Counterclockwise: the image's top-left goes to the bottom-left.
        let [tl, ..] = r.corners();
        assert_eq!(tl, Vec3::new(7.5, 0.0, 0.0));
        let corners = r.corners();
        let (lo, hi) = corners.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &c| {
            (lo.min(c), hi.max(c))
        });
        assert_eq!(
            (lo, hi),
            (Vec3::new(7.5, 0.0, 0.0), Vec3::new(12.5, 10.0, 0.0))
        );
    }

    #[test]
    fn json_round_trip_and_defaults() {
        let bounds = BoundSize::new(16, 16, 16);
        let r = ReferenceImage::fitted(3, "a", image(4, 2), RefPlane::Yz, bounds);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""data":"AQID""#), "{json}");
        let back: ReferenceImage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.image.bytes(), [1, 2, 3]);
        let minimal =
            r#"{"id":1,"name":"m","image":{"width":1,"height":1,"data":""},"size":[1,1]}"#;
        let m: ReferenceImage = serde_json::from_str(minimal).unwrap();
        assert!(m.visible && m.plane == RefPlane::Xy && m.opacity == 0.5);
        let bad = minimal.replace(r#""data":"""#, r#""data":"*!""#);
        assert!(serde_json::from_str::<ReferenceImage>(&bad).is_err());
    }

    #[test]
    fn validity() {
        let bounds = BoundSize::new(16, 16, 16);
        let mut r = ReferenceImage::fitted(1, "a", image(4, 2), RefPlane::Xy, bounds);
        assert!(r.is_valid() && r.within(100.0));
        r.size.x = 0.0;
        assert!(!r.is_valid());
        r.size.x = 1.0;
        r.offset.y = f32::NAN;
        assert!(!r.is_valid());
        r.offset.y = 1e9;
        assert!(r.is_valid() && !r.within(100.0));
        assert!(SliceFilter::SliceOnly.shows(true) && !SliceFilter::SliceOnly.shows(false));
    }
}
