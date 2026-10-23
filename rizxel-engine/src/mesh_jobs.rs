//! Background tile meshing (Phase 9).
//!
//! [`MeshJobs`] is a small pool of worker threads (`std::thread` and a
//! condition variable, no async runtime) that meshes single tiles with
//! [`mesh_tile`]. [`crate::TileMeshCache`] sends it the tiles of large
//! updates (imports, undo of big edits) and keeps drawing the last mesh of
//! each tile until the new one arrives; small edits never come here.
//!
//! The queue holds at most one job per `(cache, tile)`: a newer job for the
//! same tile replaces the queued one, so a burst of edits cannot pile up
//! stale work. Results carry the generation they were requested at and the
//! cache drops any that a later request superseded.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use glam::IVec3;
use rizxel_math::{Palette, VoxelVolume};

use crate::mesher::{mesh_tile, MaterialSlots, Mesh, MeshParams};

/// Owned copy of [`MeshParams`] that a worker thread can keep.
#[derive(Debug, Clone)]
pub(crate) struct OwnedParams {
    pub palette: Palette,
    pub slots: OwnedSlots,
    pub ao: bool,
}

/// Owned [`MaterialSlots`].
#[derive(Debug, Clone)]
pub(crate) enum OwnedSlots {
    Uniform(u8),
    PerVoxel(Arc<VoxelVolume>),
}

impl OwnedParams {
    pub fn new(params: &MeshParams<'_>) -> Self {
        Self {
            palette: params.palette.clone(),
            slots: match params.slots {
                MaterialSlots::Uniform(s) => OwnedSlots::Uniform(s),
                MaterialSlots::PerVoxel(v) => OwnedSlots::PerVoxel(Arc::new(v.clone())),
            },
            ao: params.ao,
        }
    }

    pub fn params(&self) -> MeshParams<'_> {
        let slots = match &self.slots {
            OwnedSlots::Uniform(s) => MaterialSlots::Uniform(*s),
            OwnedSlots::PerVoxel(v) => MaterialSlots::PerVoxel(v),
        };
        MeshParams::new(&self.palette).slots(slots).ao(self.ao)
    }
}

/// A finished tile: `(origin, generation, mesh)`.
pub(crate) type TileResult = (IVec3, u64, Mesh);

/// One tile to mesh.
pub(crate) struct Job {
    /// The requesting cache ([`next_cache_id`]).
    pub cache: u64,
    pub origin: IVec3,
    pub generation: u64,
    pub volume: Arc<VoxelVolume>,
    pub params: Arc<OwnedParams>,
    pub reply: Sender<TileResult>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
}

/// Worker pool for tile meshing. One shared pool serves every document
/// ([`MeshJobs::shared`]).
pub struct MeshJobs {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    workers: usize,
}

static NEXT_CACHE_ID: AtomicU64 = AtomicU64::new(1);

/// Unique id for a cache that submits jobs.
pub(crate) fn next_cache_id() -> u64 {
    NEXT_CACHE_ID.fetch_add(1, Ordering::Relaxed)
}

impl MeshJobs {
    /// Starts `workers` threads (at least one).
    pub fn new(workers: usize) -> Self {
        let workers = workers.max(1);
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        for i in 0..workers {
            let queue = queue.clone();
            std::thread::Builder::new()
                .name(format!("rizxel-mesh-{i}"))
                .spawn(move || worker(&queue))
                .expect("spawn mesh worker");
        }
        Self { queue, workers }
    }

    /// The process-wide pool: one worker per core minus the UI thread,
    /// between 1 and 4.
    pub fn shared() -> &'static MeshJobs {
        static POOL: OnceLock<MeshJobs> = OnceLock::new();
        POOL.get_or_init(|| {
            let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
            MeshJobs::new(cores.saturating_sub(1).clamp(1, 4))
        })
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Jobs waiting for a worker.
    pub fn queued(&self) -> usize {
        self.queue.0.lock().map_or(0, |q| q.jobs.len())
    }

    /// Queues `jobs`, replacing queued jobs for the same cache and tile
    /// (one lock for all: an import submits thousands).
    pub(crate) fn submit_all(&self, jobs: Vec<Job>) {
        let (lock, ready) = &*self.queue;
        let Ok(mut queue) = lock.lock() else {
            return;
        };
        let replaced: HashSet<(u64, IVec3)> = jobs.iter().map(|j| (j.cache, j.origin)).collect();
        queue
            .jobs
            .retain(|j| !replaced.contains(&(j.cache, j.origin)));
        queue.jobs.extend(jobs);
        ready.notify_all();
    }

    /// Drops every queued job of `cache` (it was rebuilt or dropped).
    pub(crate) fn cancel(&self, cache: u64) {
        if let Ok(mut queue) = self.queue.0.lock() {
            queue.jobs.retain(|j| j.cache != cache);
        }
    }
}

fn worker(queue: &(Mutex<Queue>, Condvar)) {
    let (lock, ready) = queue;
    loop {
        let job = {
            let Ok(mut q) = lock.lock() else {
                return;
            };
            loop {
                if let Some(job) = q.jobs.pop_front() {
                    break job;
                }
                q = match ready.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        let mesh = mesh_tile(&job.volume, job.origin, &job.params.params());
        // The cache may be gone; then nobody wants the result.
        let _ = job.reply.send((job.origin, job.generation, mesh));
    }
}
