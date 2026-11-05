//! Handler mode: trier running as `Trier.app`, launched by Launch Services
//! with URLs to open.
//!
//! Entered when `TRIER_HANDLER=1`, which only the bundle's `LSEnvironment`
//! sets. The routing itself lives in [`crate::dispatch`].

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};

use crate::dispatch::dispatch;
use crate::macos;
use crate::paths;

/// What the watchdog decides on. One lock for both fields, held by the
/// watchdog through `exit`, so a batch that starts routing can never be cut
/// off halfway.
struct Activity {
    /// When the last batch started or finished; `None` until one arrives.
    last_event: Option<Instant>,
    /// Whether a batch is being routed right now.
    routing: bool,
}

static ACTIVITY: Mutex<Activity> = Mutex::new(Activity {
    last_event: None,
    routing: false,
});

/// Whether any URL so far failed to route normally; becomes the exit code.
static FAILED: AtomicBool = AtomicBool::new(false);

/// How long a launch without URLs (e.g. a double-click on the bundle) may
/// keep the process alive.
const WATCHDOG: Duration = Duration::from_secs(5);

/// How long to keep accepting URLs after the last batch.
///
/// Not exiting right after routing: Launch Services hands links clicked in
/// quick succession to the already running instance, and an instance that
/// exits immediately would drop them.
const IDLE: Duration = Duration::from_secs(1);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TrierAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            set_routing(true);
            let inputs: Vec<String> = urls
                .to_vec()
                .into_iter()
                .filter_map(|url| url.absoluteString())
                .map(|url| url.to_string())
                .collect();
            let ok = dispatch(
                &inputs,
                &paths::config_path(None).unwrap_or_default(),
                paths::state_file(),
                &mut macos::open_url_with,
                &mut |message: &str| macos::show_alert("trier", message),
            );
            log::logger().flush();
            if !ok {
                FAILED.store(true, Ordering::SeqCst);
            }
            set_routing(false);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `NSObject`'s designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// Runs the handler until Launch Services delivers URLs, then exits.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn run() -> ! {
    init_logging();
    let mtm = MainThreadMarker::new().expect("handler mode runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let started = Instant::now();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let activity = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
            match activity.last_event {
                None if started.elapsed() >= WATCHDOG => {
                    log::info!("no URL received");
                    exit(false);
                }
                Some(at) if at.elapsed() >= IDLE && !activity.routing => {
                    exit(FAILED.load(Ordering::SeqCst));
                }
                _ => {}
            }
        }
    });

    app.run();
    exit(FAILED.load(Ordering::SeqCst))
}

fn set_routing(routing: bool) {
    let mut activity = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
    activity.routing = routing;
    activity.last_event = Some(Instant::now());
}

/// Not `NSApplication::terminate`: it runs the delegate shutdown machinery;
/// a plain exit keeps the shutdown deterministic.
fn exit(failed: bool) -> ! {
    log::logger().flush();
    std::process::exit(if failed { 1 } else { 0 })
}

/// Logs to `<state dir>/trier.log`; level from `TRIER_LOG` (default `info`).
///
/// A log file that cannot be opened disables logging rather than stopping
/// the click from being routed.
#[cfg_attr(coverage_nightly, coverage(off))]
fn init_logging() {
    let Ok(path) = paths::log_file() else {
        return;
    };
    let Ok(file) = open_log(&path) else {
        return;
    };
    env_logger::Builder::from_env(env_logger::Env::new().filter_or("TRIER_LOG", "info"))
        .target(env_logger::Target::Pipe(Box::new(file)))
        .init();
}

/// Opens the log for appending, readable only by the user: it records every
/// clicked URL, and URLs can carry login tokens.
///
/// The mode is also reset on an existing file, since `mode` only applies
/// when the file is created.
fn open_log(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn new_log_is_readable_only_by_the_user() {
        let path = test_support::temp_dir("handler").join("logs/trier.log");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        open_log(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn existing_log_is_made_readable_only_by_the_user() {
        let path = test_support::temp_dir("handler").join("existing/trier.log");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        open_log(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
