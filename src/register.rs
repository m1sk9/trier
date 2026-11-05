//! `Trier.app`: generating the bundle and registering it as the default
//! browser.
//!
//! The bundle is generated under `~/Applications` instead of shipping in the
//! release archive: the Homebrew Cellar path changes on every upgrade, which
//! would leave the Launch Services registration pointing at a removed path.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::browser::BundleId;
use crate::cli::Report;
use crate::macos::{self, MacosError};
use crate::paths::{self, PathsError};
use crate::state::{State, StateError};

pub const BUNDLE_ID: &str = "dev.m1sk9.trier";

const INFO_PLIST_TEMPLATE: &str = include_str!("../assets/Info.plist.template");
const SCHEMES: [&str; 2] = ["http", "https"];
const SETTINGS_HINT: &str =
    "choose the default browser under System Settings > Desktop & Dock > Default web browser";

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("cannot locate ~/Applications: HOME is not set")]
    NoHome,
    #[error(transparent)]
    Paths(#[from] PathsError),
    #[error("{} exists but is not a trier bundle; refusing to touch it", .0.display())]
    NotOurs(PathBuf),
    #[error("failed to write {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("cannot locate the trier executable: {0}")]
    CurrentExe(io::Error),
    #[error(
        "{} runs from inside {}, which `register` replaces; run a trier installed elsewhere",
        exe.display(),
        bundle.display()
    )]
    ExeInBundle { exe: PathBuf, bundle: PathBuf },
    #[error(transparent)]
    Macos(#[from] MacosError),
    #[error(transparent)]
    State(#[from] StateError),
}

pub fn own_bundle_id() -> BundleId {
    BundleId::try_from(BUNDLE_ID.to_owned()).expect("BUNDLE_ID is not empty")
}

/// `~/Applications/Trier.app` under `home`.
pub fn bundle_path(home: &Path) -> PathBuf {
    home.join("Applications/Trier.app")
}

/// `~/Applications/Trier.app` for the current user.
pub fn installed_bundle_path() -> Result<PathBuf, RegisterError> {
    paths::home_dir()
        .map(|home| bundle_path(&home))
        .ok_or(RegisterError::NoHome)
}

/// Refuses an `exe` that lives inside `bundle`: `register` removes the
/// bundle before copying `exe` into the new one.
pub fn check_exe_outside(exe: &Path, bundle: &Path) -> Result<(), RegisterError> {
    let bundle = bundle.canonicalize().unwrap_or_else(|_| bundle.to_owned());
    if exe.starts_with(&bundle) {
        return Err(RegisterError::ExeInBundle {
            exe: exe.to_owned(),
            bundle,
        });
    }
    Ok(())
}

/// The environment Launch Services gives handler mode.
///
/// Launch Services launches inherit launchd's environment, not the shell's,
/// so the config and state locations `register` resolved (from `--config`,
/// `TRIER_CONFIG`, `XDG_*`) are written into the bundle; otherwise a click
/// would read different files than `trier check` / `status` just did.
pub fn handler_environment(config_path: &Path, state_dir: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        (macos::HANDLER_ENV.to_owned(), "1".to_owned()),
        (
            paths::CONFIG_ENV.to_owned(),
            config_path.display().to_string(),
        ),
    ];
    if let Some(state_home) = state_dir.parent() {
        env.push((
            "XDG_STATE_HOME".to_owned(),
            state_home.display().to_string(),
        ));
    }
    env
}

/// `Info.plist` for a bundle identified as `bundle_id` running trier
/// `version` with `env` as its `LSEnvironment`. Production passes
/// [`BUNDLE_ID`].
pub fn render_info_plist(bundle_id: &str, version: &str, env: &[(String, String)]) -> String {
    let environment: String = env
        .iter()
        .map(|(key, value)| {
            format!(
                "\t\t<key>{}</key>\n\t\t<string>{}</string>\n",
                xml_escape(key),
                xml_escape(value)
            )
        })
        .collect();
    INFO_PLIST_TEMPLATE
        .replace("{{BUNDLE_ID}}", bundle_id)
        .replace("{{VERSION}}", version)
        .replace("{{ENVIRONMENT}}", &environment)
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Whether `bundle` is a bundle trier generated, judged by its `Info.plist`.
pub fn is_trier_bundle(bundle: &Path) -> bool {
    std::fs::read_to_string(bundle.join("Contents/Info.plist"))
        .is_ok_and(|plist| plist.contains(&format!("<string>{BUNDLE_ID}</string>")))
}

/// Removes `bundle` if it exists; returns whether it did.
///
/// Refuses anything trier did not generate, so a user's own app that
/// happens to sit at the same path is never deleted.
pub fn remove_bundle(bundle: &Path) -> Result<bool, RegisterError> {
    if !bundle.exists() {
        return Ok(false);
    }
    if !is_trier_bundle(bundle) {
        return Err(RegisterError::NotOurs(bundle.to_owned()));
    }
    std::fs::remove_dir_all(bundle).map_err(|source| RegisterError::Io {
        path: bundle.to_owned(),
        source,
    })?;
    Ok(true)
}

/// Removes `bundle` from Launch Services with `unregister`, then deletes it;
/// returns whether it existed.
///
/// Ownership is checked before `unregister` runs, not left to
/// [`remove_bundle`]: by then a user's own app at the same path would
/// already have lost its Launch Services registration.
pub fn retire_bundle(
    bundle: &Path,
    unregister: impl FnOnce(&Path) -> Result<(), MacosError>,
    report: &mut Report,
) -> Result<bool, RegisterError> {
    if !bundle.exists() {
        return Ok(false);
    }
    if !is_trier_bundle(bundle) {
        return Err(RegisterError::NotOurs(bundle.to_owned()));
    }
    if let Err(e) = unregister(bundle) {
        report.err(format_args!("warning: {e}"));
    }
    remove_bundle(bundle)
}

/// Writes `Contents/{Info.plist,PkgInfo,MacOS/trier}` under `bundle`, with a
/// copy of `exe` as the executable.
pub fn write_bundle(
    bundle: &Path,
    exe: &Path,
    bundle_id: &str,
    version: &str,
    env: &[(String, String)],
) -> Result<(), RegisterError> {
    let contents = bundle.join("Contents");
    let executable = contents.join("MacOS/trier");
    let io_err = |path: &Path| {
        let path = path.to_owned();
        move |source| RegisterError::Io { path, source }
    };
    std::fs::create_dir_all(contents.join("MacOS")).map_err(io_err(&contents))?;
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, render_info_plist(bundle_id, version, env)).map_err(io_err(&plist))?;
    let pkg_info = contents.join("PkgInfo");
    std::fs::write(&pkg_info, "APPL????").map_err(io_err(&pkg_info))?;
    std::fs::copy(exe, &executable).map_err(io_err(&executable))?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
        .map_err(io_err(&executable))
}

/// `trier register`: generates, signs and registers `Trier.app`, records the
/// current default browser, and makes trier the default.
///
/// Returns `false` only when the bundle could not be put in place; failing
/// to change the default is a warning, because the user can still pick
/// Trier in System Settings.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn register(config_path: &Path, report: &mut Report) -> bool {
    match try_register(config_path, report) {
        Ok(()) => true,
        Err(e) => {
            report.err(format_args!("error: {e}"));
            false
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn try_register(config_path: &Path, report: &mut Report) -> Result<(), RegisterError> {
    let bundle = installed_bundle_path()?;
    let state_path = paths::state_file()?;
    let config_path = std::path::absolute(config_path).map_err(|source| RegisterError::Io {
        path: config_path.to_owned(),
        source,
    })?;
    let env = handler_environment(&config_path, &paths::state_dir()?);
    let own = own_bundle_id();

    let exe = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(RegisterError::CurrentExe)?;
    check_exe_outside(&exe, &bundle)?;
    if remove_bundle(&bundle)? {
        report.out(format_args!("Removed the old {}", bundle.display()));
    }
    report.out(format_args!("Writing {}", bundle.display()));
    write_bundle(&bundle, &exe, BUNDLE_ID, env!("CARGO_PKG_VERSION"), &env)?;
    report.out(format_args!(
        "Clicked links will use the config at {}",
        config_path.display()
    ));
    report.out("Signing the bundle (ad hoc)");
    macos::codesign(&bundle, &own)?;
    report.out("Registering the bundle with Launch Services");
    macos::lsregister(&bundle, false)?;

    let mut state = State::load(&state_path).unwrap_or_else(|e| {
        report.err(format_args!("warning: starting from an empty state: {e}"));
        State::default()
    });
    let current = macos::default_https_handler().map(|(_, id)| id);
    state.record_previous_default(current.as_ref(), &own);
    state.registered_version = Some(env!("CARGO_PKG_VERSION").to_owned());
    state.save(&state_path)?;
    match &state.previous_default_browser_bundle_id {
        Some(previous) => report.out(format_args!(
            "Recorded the previous default browser: {previous}"
        )),
        None => report.out("No previous default browser to record"),
    }

    report.out("Making trier the default browser for http and https; macOS may ask you to confirm");
    let mut all_set = true;
    for scheme in SCHEMES {
        if let Err(e) = macos::set_default_handler(scheme, &own) {
            report.err(format_args!("warning: {e}"));
            all_set = false;
        }
    }
    if !all_set {
        report.err(format_args!("warning: {SETTINGS_HINT}"));
    }
    match macos::default_https_handler() {
        Some((_, id)) if id == own => report.out("trier is now the default browser"),
        Some((path, id)) => {
            report.out(format_args!(
                "The default https handler is still {id} ({})",
                path.display()
            ));
            report.out(
                "If macOS asked for confirmation, the change applies once you accept; check with `trier status`",
            );
        }
        None => report.out("There is no default https handler yet; check with `trier status`"),
    }
    Ok(())
}

/// `trier unregister`: restores the recorded default browser and removes
/// `Trier.app`.
///
/// The recorded previous default is kept so `explain` / `open` still have a
/// fallback until the config sets `default_browser`.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn unregister(report: &mut Report) -> bool {
    match try_unregister(report) {
        Ok(()) => true,
        Err(e) => {
            report.err(format_args!("error: {e}"));
            false
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn try_unregister(report: &mut Report) -> Result<(), RegisterError> {
    let bundle = installed_bundle_path()?;
    let state_path = paths::state_file()?;
    // A broken state must not lock trier into the default-browser slot.
    let mut state = State::load(&state_path).unwrap_or_else(|e| {
        report.err(format_args!("warning: starting from an empty state: {e}"));
        State::default()
    });

    let current = macos::default_https_handler()
        .map(|(_, id)| id)
        .filter(|id| id.as_str() != BUNDLE_ID);
    if let Some(current) = current {
        // The user picked another browser since registering; keep it.
        report.out(format_args!(
            "trier is no longer the default browser (now {current}); leaving it"
        ));
    } else {
        match &state.previous_default_browser_bundle_id {
            Some(previous) if macos::app_path_for_bundle_id(previous).is_some() => {
                report.out(format_args!(
                    "Restoring {previous} as the default browser; macOS may ask you to confirm"
                ));
                for scheme in SCHEMES {
                    if let Err(e) = macos::set_default_handler(scheme, previous) {
                        report.err(format_args!("warning: {e}; {SETTINGS_HINT}"));
                    }
                }
            }
            Some(previous) => report.err(format_args!(
                "warning: the previous default browser {previous} is not installed; {SETTINGS_HINT}"
            )),
            None => report.err(format_args!(
                "warning: no previous default browser is recorded; {SETTINGS_HINT}"
            )),
        }
    }

    if retire_bundle(&bundle, |b| macos::lsregister(b, true), report)? {
        report.out(format_args!("Removed {}", bundle.display()));
    } else {
        report.out(format_args!("{} does not exist", bundle.display()));
    }

    state.registered_version = None;
    state.save(&state_path)?;
    if let Some(previous) = &state.previous_default_browser_bundle_id {
        report.out(format_args!(
            "Kept {previous} as the recorded previous default in {}",
            state_path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::test_support;

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = test_support::temp_dir("register").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Not trier's real identifier: macOS registers any complete app bundle
    /// it finds, and one carrying `BUNDLE_ID` would receive clicked links.
    const TEST_BUNDLE_ID: &str = "dev.m1sk9.trier.test";

    /// Deletes its directory on drop, so a failed assertion does not leave a
    /// complete bundle behind for Launch Services to register.
    struct RemoveOnDrop(PathBuf);

    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn env() -> Vec<(String, String)> {
        handler_environment(
            Path::new("/Users/me/.config/trier/config.toml"),
            Path::new("/Users/me/.local/state/trier"),
        )
    }

    fn fake_exe(dir: &Path) -> PathBuf {
        let exe = dir.join("trier-bin");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        exe
    }

    #[test]
    fn info_plist_identifies_trier_and_its_version() {
        let plist = render_info_plist(BUNDLE_ID, "1.2.3", &env());
        assert!(plist.contains("<string>dev.m1sk9.trier</string>"));
        assert_eq!(plist.matches("<string>1.2.3</string>").count(), 2);
        assert!(!plist.contains("{{"), "unreplaced placeholder");
    }

    #[test]
    fn info_plist_claims_http_and_https() {
        let plist = render_info_plist(BUNDLE_ID, "1.2.3", &env());
        assert!(plist.contains("<string>http</string>"));
        assert!(plist.contains("<string>https</string>"));
    }

    #[test]
    fn info_plist_runs_without_a_dock_icon_in_handler_mode() {
        let plist = render_info_plist(BUNDLE_ID, "1.2.3", &env());
        assert!(plist.contains("<key>LSUIElement</key>\n\t<true/>"));
        assert!(plist.contains(
            "<key>LSEnvironment</key>\n\t<dict>\n\t\t<key>TRIER_HANDLER</key>\n\t\t<string>1</string>\n"
        ));
    }

    #[test]
    fn bundle_lives_in_the_user_applications_folder() {
        assert_eq!(
            bundle_path(Path::new("/Users/me")),
            Path::new("/Users/me/Applications/Trier.app")
        );
    }

    #[test]
    fn written_bundle_has_plist_pkginfo_and_executable() {
        let dir = fresh_dir("layout");
        let _cleanup = RemoveOnDrop(dir.clone());
        let bundle = test_support::test_bundle(&dir);
        write_bundle(&bundle, &fake_exe(&dir), TEST_BUNDLE_ID, "1.2.3", &env()).unwrap();

        let contents = bundle.join("Contents");
        assert_eq!(
            std::fs::read_to_string(contents.join("Info.plist")).unwrap(),
            render_info_plist(TEST_BUNDLE_ID, "1.2.3", &env())
        );
        assert_eq!(
            std::fs::read_to_string(contents.join("PkgInfo")).unwrap(),
            "APPL????"
        );
        let mode = std::fs::metadata(contents.join("MacOS/trier"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn removes_a_bundle_trier_generated() {
        let dir = fresh_dir("remove-ours");
        let bundle = test_support::fake_trier_bundle(&dir);
        assert!(remove_bundle(&bundle).unwrap());
        assert!(!bundle.exists());
    }

    #[test]
    fn refuses_to_remove_a_bundle_without_our_identifier() {
        let dir = fresh_dir("remove-foreign");
        let bundle = test_support::foreign_bundle(&dir);
        assert!(matches!(
            remove_bundle(&bundle),
            Err(RegisterError::NotOurs(_))
        ));
        assert!(bundle.exists());
    }

    #[test]
    fn refuses_to_remove_a_bundle_without_info_plist() {
        let dir = fresh_dir("remove-no-plist");
        let bundle = test_support::test_bundle(&dir);
        std::fs::create_dir_all(&bundle).unwrap();
        assert!(matches!(
            remove_bundle(&bundle),
            Err(RegisterError::NotOurs(_))
        ));
    }

    #[test]
    fn removing_a_missing_bundle_is_a_no_op() {
        let dir = fresh_dir("remove-missing");
        assert!(!remove_bundle(&test_support::test_bundle(&dir)).unwrap());
    }

    #[test]
    fn retiring_our_bundle_unregisters_then_removes_it() {
        let dir = fresh_dir("retire-ours");
        let bundle = test_support::fake_trier_bundle(&dir);
        let mut unregistered = Vec::new();
        let mut report = Report::default();
        let removed = retire_bundle(
            &bundle,
            |b| {
                unregistered.push(b.to_owned());
                Ok(())
            },
            &mut report,
        )
        .unwrap();
        assert!(removed);
        assert_eq!(unregistered, std::slice::from_ref(&bundle));
        assert!(!bundle.exists());
    }

    #[test]
    fn retiring_a_foreign_bundle_leaves_launch_services_alone() {
        let dir = fresh_dir("retire-foreign");
        let bundle = test_support::foreign_bundle(&dir);
        let mut unregistered = false;
        let result = retire_bundle(
            &bundle,
            |_| {
                unregistered = true;
                Ok(())
            },
            &mut Report::default(),
        );
        assert!(matches!(result, Err(RegisterError::NotOurs(_))));
        assert!(!unregistered);
        assert!(bundle.exists());
    }

    #[test]
    fn failing_to_unregister_still_removes_the_bundle_with_a_warning() {
        let dir = fresh_dir("retire-unregister-fails");
        let bundle = test_support::fake_trier_bundle(&dir);
        let mut report = Report::default();
        let removed = retire_bundle(
            &bundle,
            |_| {
                Err(MacosError::Spawn {
                    program: "fake-lsregister".to_owned(),
                    source: io::Error::other("refused"),
                })
            },
            &mut report,
        )
        .unwrap();
        assert!(removed);
        assert!(!bundle.exists());
        assert!(
            report
                .stderr()
                .starts_with("warning: failed to run fake-lsregister"),
            "{}",
            report.stderr()
        );
    }

    #[test]
    fn retiring_a_missing_bundle_does_not_unregister() {
        let dir = fresh_dir("retire-missing");
        let removed = retire_bundle(
            &test_support::test_bundle(&dir),
            |_| panic!("nothing to unregister"),
            &mut Report::default(),
        )
        .unwrap();
        assert!(!removed);
    }

    #[test]
    fn refuses_an_executable_inside_the_bundle() {
        let dir = fresh_dir("exe-inside");
        let bundle = test_support::fake_trier_bundle(&dir);
        let exe = bundle.canonicalize().unwrap().join("Contents/MacOS/trier");
        assert!(matches!(
            check_exe_outside(&exe, &bundle),
            Err(RegisterError::ExeInBundle { .. })
        ));
    }

    #[test]
    fn accepts_an_executable_outside_the_bundle() {
        let dir = fresh_dir("exe-outside");
        let bundle = test_support::fake_trier_bundle(&dir);
        let exe = dir.canonicalize().unwrap().join("trier");
        check_exe_outside(&exe, &bundle).unwrap();
        check_exe_outside(&exe, &dir.join("missing/Trier.app")).unwrap();
    }

    #[test]
    fn handler_environment_carries_the_resolved_config_and_state_locations() {
        assert_eq!(
            env(),
            [
                ("TRIER_HANDLER".to_owned(), "1".to_owned()),
                (
                    "TRIER_CONFIG".to_owned(),
                    "/Users/me/.config/trier/config.toml".to_owned()
                ),
                (
                    "XDG_STATE_HOME".to_owned(),
                    "/Users/me/.local/state".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn info_plist_passes_the_handler_environment() {
        let plist = render_info_plist(BUNDLE_ID, "1.2.3", &env());
        assert!(plist.contains(
            "\t\t<key>TRIER_CONFIG</key>\n\t\t<string>/Users/me/.config/trier/config.toml</string>\n"
        ));
        assert!(plist.contains("<string>/Users/me/.local/state</string>"));
    }

    #[test]
    fn environment_values_are_xml_escaped() {
        let env = handler_environment(Path::new("/tmp/a&b<c>.toml"), Path::new("/s/trier"));
        let plist = render_info_plist(BUNDLE_ID, "1.2.3", &env);
        assert!(plist.contains("<string>/tmp/a&amp;b&lt;c&gt;.toml</string>"));
    }

    #[test]
    fn info_plist_does_not_claim_handoff() {
        assert!(!render_info_plist(BUNDLE_ID, "1.2.3", &env()).contains("NSUserActivityTypes"));
    }
}
