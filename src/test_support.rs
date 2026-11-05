use std::path::{Path, PathBuf};

use crate::browser::BundleId;

/// A state file path that never exists.
pub const NO_STATE: &str = "/nonexistent/trier/state.toml";

/// The example config shipped with the crate.
pub fn example_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/config.toml.example")
}

/// A per-module scratch directory under `target/tmp`.
pub fn temp_dir(module: &str) -> PathBuf {
    // Not `std::env::temp_dir()`: it reads TMPDIR at runtime, racing with
    // the env-mutating tests in `paths`.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tmp")
        .join(module);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `content` to `name` inside `temp_dir(module)` and returns its path.
pub fn temp_file(module: &str, name: &str, content: &str) -> PathBuf {
    let path = temp_dir(module).join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// An app lookup where only `installed` is installed, under `/Applications`.
pub fn lookup_installed(
    installed: &'static [&'static str],
) -> impl Fn(&BundleId) -> Option<PathBuf> {
    move |id| {
        installed
            .contains(&id.as_str())
            .then(|| PathBuf::from(format!("/Applications/{id}.app")))
    }
}

/// An app lookup where every app is installed, under `/Applications`.
pub fn all_installed(id: &BundleId) -> Option<PathBuf> {
    Some(PathBuf::from(format!("/Applications/{id}.app")))
}
