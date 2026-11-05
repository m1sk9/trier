//! The only module that touches macOS APIs or macOS-only executables.
//!
//! On other platforms every function returns [`MacosError::Unsupported`], so
//! callers can tell "not available here" apart from "not installed".

use std::ffi::OsString;
use std::process::ExitStatus;

use url::Url;

use crate::browser::BundleId;

#[derive(Debug, thiserror::Error)]
pub enum MacosError {
    #[error("not supported on this platform (macOS only)")]
    Unsupported,
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

pub use imp::*;

#[cfg(target_os = "macos")]
#[cfg_attr(coverage_nightly, coverage(off))]
mod imp {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::process::{Command, Output};

    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSString, NSURL};
    use url::Url;

    use super::{MacosError, alert_command, open_command};
    use crate::browser::BundleId;

    /// Where the app with `id` is installed, if anywhere.
    pub fn app_path_for_bundle_id(id: &BundleId) -> Result<Option<PathBuf>, MacosError> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(id.as_str()));
        Ok(app
            .and_then(|url| url.path())
            .map(|p| PathBuf::from(p.to_string())))
    }

    /// The app currently registered to open `https` URLs.
    pub fn default_https_handler() -> Result<Option<(PathBuf, BundleId)>, MacosError> {
        let Some(probe) = NSURL::URLWithString(&NSString::from_str("https://example.com")) else {
            return Ok(None);
        };
        let Some(app) = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&probe) else {
            return Ok(None);
        };
        let path = app.path().map(|p| PathBuf::from(p.to_string()));
        let id = NSBundle::bundleWithURL(&app)
            .and_then(|bundle| bundle.bundleIdentifier())
            .and_then(|id| BundleId::try_from(id.to_string()).ok());
        Ok(path.zip(id))
    }

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
    pub fn show_alert(title: &str, message: &str) {
        let _ = run(alert_command(title, message));
    }

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
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::PathBuf;

    use url::Url;

    use super::MacosError;
    use crate::browser::BundleId;

    pub fn app_path_for_bundle_id(_id: &BundleId) -> Result<Option<PathBuf>, MacosError> {
        Err(MacosError::Unsupported)
    }

    pub fn default_https_handler() -> Result<Option<(PathBuf, BundleId)>, MacosError> {
        Err(MacosError::Unsupported)
    }

    pub fn open_url_with(_bundle_id: &BundleId, _url: &Url) -> Result<(), MacosError> {
        Err(MacosError::Unsupported)
    }

    pub fn show_alert(_title: &str, _message: &str) {}
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

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_safari() {
        let path = app_path_for_bundle_id(&bundle_id("com.apple.Safari"))
            .unwrap()
            .expect("Safari ships with macOS");
        assert!(path.ends_with("Safari.app"), "{}", path.display());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unknown_bundle_id_is_not_installed() {
        assert_eq!(
            app_path_for_bundle_id(&bundle_id("invalid.trier.does-not-exist")).unwrap(),
            None
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn lookups_are_unsupported_off_macos() {
        assert!(matches!(
            app_path_for_bundle_id(&bundle_id("com.apple.Safari")),
            Err(MacosError::Unsupported)
        ));
    }
}
