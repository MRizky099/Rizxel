//! Optional sample files for tests that compare against real-world files.
//!
//! `RIZXEL_SAMPLES` lists the folders to search, separated like `PATH`
//! (`;` on Windows, `:` elsewhere). Tests that need a sample return early
//! when the variable is unset or the file is missing, so CI and fresh
//! clones skip them.

use std::path::PathBuf;

/// Environment variable naming the sample folders.
pub const SAMPLES_ENV: &str = "RIZXEL_SAMPLES";

/// The first `RIZXEL_SAMPLES` folder that contains `name`, joined with it.
pub fn sample(name: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os(SAMPLES_ENV)?;
    std::env::split_paths(&dirs)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}
