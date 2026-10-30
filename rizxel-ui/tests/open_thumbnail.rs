//! Opening a project writes its start-screen thumbnail in the same frame
//! the viewport starts meshing it in the background. The thumbnail must
//! not take the viewport's waiting tiles (the model then stayed missing
//! until an edit). Own test binary: it sets `RIZXEL_DATA_DIR`.

use std::time::{Duration, Instant};

use glam::IVec3;
use rizxel_app::{AppPaths, AppState, Document};
use rizxel_math::BoundSize;
use rizxel_ui::render::offscreen::headless_device;
use rizxel_ui::thumbnails;
use rizxel_ui::viewport::MeshCache;

#[test]
fn opened_model_shows_after_its_thumbnail_renders() {
    let Some(gpu) = headless_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rizxel_open_thumb_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("RIZXEL_DATA_DIR", dir.join("data"));

    // 32³ with voxels in all 8 tiles of 16³: more than meshes synchronously.
    let mut doc = Document::new("icon");
    doc.set_bounds(BoundSize::new(32, 32, 32), false);
    let cubes = (0..8).flat_map(|t: i32| {
        let o = IVec3::new(t & 1, (t >> 1) & 1, (t >> 2) & 1) * 16 + IVec3::splat(4);
        (0..27).map(move |i| (o + IVec3::new(i % 3, i / 3 % 3, i / 9), 5))
    });
    doc.set_voxels(0, cubes, false);
    let file = dir.join("icon.rzxl");
    doc.save_to(&file).unwrap();

    let mut state = AppState::with_paths(AppPaths::new());
    state.open(&file).unwrap();
    assert_eq!(state.thumbnail_requests.len(), 1);

    // One frame as `app.rs` runs it: the viewport, then the thumbnails.
    let mut cache = MeshCache::default();
    let first = cache.get(state.doc().unwrap(), false);
    assert!(
        first.opaque.indices.is_empty(),
        "the first mesh goes to the pool"
    );
    let mut raster = None;
    thumbnails::update_with(&mut state, Some((&gpu.device, &gpu.queue)), &mut raster);
    let paths = state.paths().unwrap();
    assert!(paths.thumbnail_file(&file).exists());

    // Later frames take in the finished tiles.
    let doc = state.doc().unwrap();
    let start = Instant::now();
    while doc.view_meshing() && start.elapsed() < Duration::from_secs(10) {
        cache.get(doc, false);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(300));
    let meshes = cache.get(doc, false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        !meshes.opaque.indices.is_empty(),
        "the viewport shows the opened model"
    );
}
