//! The only module that touches macOS APIs or macOS-only executables.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};
use url::Url;

use crate::browser::BundleId;

#[derive(Debug, thiserror::Error)]
pub enum MacosError {
    #[error("failed to run {program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("`open` exited with {status}: {stderr}")]
    OpenFailed { status: ExitStatus, stderr: String },
}

const OPEN: &str = "/usr/bin/open";
const OSASCRIPT: &str = "/usr/bin/osascript";

/// argv for opening `url` with the app identified by `bundle_id`.
///
/// The URL is its own argv element and no shell is involved, so it is never
/// interpreted.
pub fn open_command(bundle_id: &BundleId, url: &Url) -> Vec<OsString> {
    vec![
        OPEN.into(),
        "-b".into(),
        bundle_id.as_str().into(),
        url.as_str().into(),
    ]
}

/// argv for showing a modal alert.
///
/// `title` and `message` are passed as script arguments rather than spliced
/// into the AppleScript source, so they need no quoting.
pub fn alert_command(title: &str, message: &str) -> Vec<OsString> {
    vec![
        OSASCRIPT.into(),
        "-e".into(),
        "on run argv".into(),
        "-e".into(),
        "display alert (item 1 of argv) message (item 2 of argv)".into(),
        "-e".into(),
        "end run".into(),
        title.into(),
        message.into(),
    ]
}

/// Where the app with `id` is installed, if anywhere.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn app_path_for_bundle_id(id: &BundleId) -> Option<PathBuf> {
    let workspace = NSWorkspace::sharedWorkspace();
    workspace
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(id.as_str()))
        .and_then(|url| url.path())
        .map(|p| PathBuf::from(p.to_string()))
}

/// The app currently registered to open `https` URLs.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn default_https_handler() -> Option<(PathBuf, BundleId)> {
    let probe = NSURL::URLWithString(&NSString::from_str("https://example.com"))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&probe)?;
    let path = app.path().map(|p| PathBuf::from(p.to_string()))?;
    let id = NSBundle::bundleWithURL(&app)?.bundleIdentifier()?;
    Some((path, BundleId::try_from(id.to_string()).ok()?))
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn open_url_with(bundle_id: &BundleId, url: &Url) -> Result<(), MacosError> {
    let output = run(open_command(bundle_id, url))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(MacosError::OpenFailed {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Shows a modal alert; failures are ignored because there is no better
/// channel left to report them on.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn show_alert(title: &str, message: &str) {
    let _ = run(alert_command(title, message));
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn run(argv: Vec<OsString>) -> Result<Output, MacosError> {
    let (program, args) = argv.split_first().expect("argv has a program");
    Command::new(program)
        .args(args)
        .output()
        .map_err(|source| MacosError::Spawn {
            program: program.to_string_lossy().into_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_id(s: &str) -> BundleId {
        BundleId::try_from(s.to_owned()).unwrap()
    }

    #[test]
    fn open_command_passes_bundle_id_and_url_as_separate_arguments() {
        let url = Url::parse("https://example.com/a b?x=1&y=$(rm)").unwrap();
        assert_eq!(
            open_command(&bundle_id("com.apple.Safari"), &url),
            [
                "/usr/bin/open",
                "-b",
                "com.apple.Safari",
                "https://example.com/a%20b?x=1&y=$(rm)"
            ]
        );
    }

    #[test]
    fn alert_command_passes_text_as_script_arguments() {
        let argv = alert_command("trier \"error\"", "line 1\nline 2");
        assert_eq!(argv[0], "/usr/bin/osascript");
        assert_eq!(
            &argv[argv.len() - 2..],
            ["trier \"error\"", "line 1\nline 2"]
        );
        assert!(
            argv[..argv.len() - 2]
                .iter()
                .all(|a| !a.to_string_lossy().contains("error\""))
        );
    }

    #[test]
    fn finds_safari() {
        let path = app_path_for_bundle_id(&bundle_id("com.apple.Safari"))
            .expect("Safari ships with macOS");
        assert!(path.ends_with("Safari.app"), "{}", path.display());
    }

    #[test]
    fn unknown_bundle_id_is_not_installed() {
        assert_eq!(
            app_path_for_bundle_id(&bundle_id("invalid.trier.does-not-exist")),
            None
        );
    }
}
