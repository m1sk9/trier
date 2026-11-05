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

/// Where tests put a bundle under `dir`; not `*.app`, so Finder and
/// Spotlight do not treat it as an application.
pub fn test_bundle(dir: &Path) -> PathBuf {
    dir.join("Trier.test-bundle")
}

/// A bundle that fails trier's identity check, as a user's own app at the
/// same path would.
pub fn foreign_bundle(dir: &Path) -> PathBuf {
    let bundle = test_bundle(dir);
    std::fs::create_dir_all(bundle.join("Contents")).unwrap();
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        "<plist><dict><key>CFBundleIdentifier</key><string>com.example.other</string></dict></plist>",
    )
    .unwrap();
    bundle
}

/// A bundle that passes trier's identity check but is not an application.
///
/// Only an `Info.plist` naming trier's identifier: a complete bundle with
/// that identifier gets registered with Launch Services and would receive
/// the user's clicked links.
pub fn fake_trier_bundle(dir: &Path) -> PathBuf {
    let bundle = test_bundle(dir);
    std::fs::create_dir_all(bundle.join("Contents")).unwrap();
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        format!("<string>{}</string>", crate::register::BUNDLE_ID),
    )
    .unwrap();
    bundle
}
