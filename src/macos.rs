//! The only module that touches macOS APIs or macOS-only executables.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::CFString;
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
    #[error("`{program}` exited with {status}: {stderr}")]
    CommandFailed {
        program: String,
        status: ExitStatus,
        stderr: String,
    },
    #[error("Launch Services refused to set the `{scheme}` handler (OSStatus {status})")]
    SetDefaultHandler { scheme: String, status: i32 },
}

/// Set only by `Trier.app`'s `LSEnvironment`; selects handler mode.
pub const HANDLER_ENV: &str = "TRIER_HANDLER";

const OPEN: &str = "/usr/bin/open";
const OSASCRIPT: &str = "/usr/bin/osascript";
const CODESIGN: &str = "/usr/bin/codesign";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

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
    run_checked(open_command(bundle_id, url))
}

/// argv for ad-hoc signing `bundle` with `identifier`.
pub fn codesign_command(bundle: &Path, identifier: &BundleId) -> Vec<OsString> {
    vec![
        CODESIGN.into(),
        "--force".into(),
        "--sign".into(),
        "-".into(),
        "--identifier".into(),
        identifier.as_str().into(),
        bundle.into(),
    ]
}

/// argv for registering (or with `unregister`, removing) `bundle` in the
/// Launch Services database.
pub fn lsregister_command(bundle: &Path, unregister: bool) -> Vec<OsString> {
    vec![
        LSREGISTER.into(),
        if unregister { "-u" } else { "-f" }.into(),
        bundle.into(),
    ]
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn codesign(bundle: &Path, identifier: &BundleId) -> Result<(), MacosError> {
    run_checked(codesign_command(bundle, identifier))
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn lsregister(bundle: &Path, unregister: bool) -> Result<(), MacosError> {
    run_checked(lsregister_command(bundle, unregister))
}

/// Makes the app with `bundle_id` the handler for `scheme` URLs.
///
/// macOS may ask the user to confirm the change.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn set_default_handler(scheme: &str, bundle_id: &BundleId) -> Result<(), MacosError> {
    // Deprecated since macOS 12, but still effective (checked on macOS 27);
    // its replacement on NSWorkspace is asynchronous and needs a run loop.
    #[allow(deprecated)]
    // SAFETY: both arguments are valid CFStrings for the duration of the call.
    let status = unsafe {
        objc2_core_services::LSSetDefaultHandlerForURLScheme(
            &CFString::from_str(scheme),
            &CFString::from_str(bundle_id.as_str()),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(MacosError::SetDefaultHandler {
            scheme: scheme.to_owned(),
            status,
        })
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn run_checked(argv: Vec<OsString>) -> Result<(), MacosError> {
    let program = argv[0].to_string_lossy().into_owned();
    let output = run(argv)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(MacosError::CommandFailed {
            program,
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
    // Handler mode inherits this from `LSEnvironment`; a `trier` started
    // anywhere downstream must not enter handler mode too.
    Command::new(program)
        .args(args)
        .env_remove(HANDLER_ENV)
        .output()
        .map_err(|source| MacosError::Spawn {
            program: program.to_string_lossy().into_owned(),
            source,
        })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
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

    #[test]
    fn codesign_command_signs_ad_hoc_with_our_identifier() {
        assert_eq!(
            codesign_command(
                Path::new("/Users/me/Applications/Trier.app"),
                &bundle_id("dev.m1sk9.trier")
            ),
            [
                "/usr/bin/codesign",
                "--force",
                "--sign",
                "-",
                "--identifier",
                "dev.m1sk9.trier",
                "/Users/me/Applications/Trier.app"
            ]
        );
    }

    #[test]
    fn lsregister_command_registers_or_unregisters() {
        let bundle = Path::new("/Users/me/Applications/Trier.app");
        assert_eq!(lsregister_command(bundle, false)[1], "-f");
        assert_eq!(lsregister_command(bundle, true)[1], "-u");
        assert!(
            lsregister_command(bundle, false)[0]
                .to_string_lossy()
                .ends_with("/lsregister")
        );
    }
}
