use std::path::{Path, PathBuf};

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
