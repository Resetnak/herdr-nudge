//! Registering the notifier's bundle with Launch Services.
//!
//! macOS asks for notification permission only for an app the notification
//! service (`usernoted`) can find. Running the notifier registers it as a
//! side effect, but that registration is made by the system's Launch
//! Services and announced system-wide, and `usernoted` listens only for
//! changes announced to the user's session. On most Macs it finds the app
//! anyway. On one managed Mac it didn't: 10 of 11 fresh bundles were
//! rejected on their first run ("Failed to find or validate client"), exit
//! 3 and no prompt, and stayed rejected. Restarting `usernoted` alone made
//! the same copy prompt. Registering from the user's own account is
//! announced to the session, so we do that before the notifier first runs.
//!
//! `herdr plugin install` runs nothing of ours from the final folder: its
//! build step runs in a temporary checkout that is moved afterwards, and the
//! startup hook waits for the next server start. So every hook checks first,
//! before anything can run the notifier, a `-remove` included. A marker in
//! the state directory makes the registration itself once per install; a
//! hook after that looks up two file dates and reads one small file.
//! `--cleanup` and `doctor` register every time.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::notifier;
use crate::process::Runner;
use crate::state::{self, StateDir};

const OSASCRIPT: &str = "/usr/bin/osascript";

/// It takes about 200 ms, but `osascript` starting cold on a Mac that's
/// just logged in can take longer than the 2 s every other call gets.
/// Killed halfway, it would have to be retried by the next hook anyway.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Apple's `LSRegisterURL` with `inUpdate` true, which rebuilds the entry
/// even when Launch Services thinks it's current. Plain registration skips a
/// bundle whose date hasn't changed, which is why it didn't clear the
/// rejection. The path arrives as `argv[0]`, never as part of the script.
pub const SCRIPT: &str = "function run(argv) { ObjC.import(\"CoreServices\"); \
return $.LSRegisterURL($.NSURL.fileURLWithPath(argv[0]), true) }";

pub fn osascript_path() -> &'static Path {
    Path::new(OSASCRIPT)
}

/// Registers `bundle` now.
///
/// `osascript` exits 0 whatever the call returned, so success is the `0` it
/// prints (`tests/fixtures/sys/osascript-register-bundle.json`). A missing
/// bundle prints `-43` (`osascript-register-missing.json`).
pub fn register<R: Runner>(runner: &R, bundle: &Path) -> Result<(), String> {
    let path = bundle
        .to_str()
        .ok_or_else(|| format!("bundle path is not utf-8: {}", bundle.display()))?;
    let out = runner
        .run_with_timeout(
            osascript_path(),
            &["-l", "JavaScript", "-e", SCRIPT, path],
            TIMEOUT,
        )
        .map_err(|e| format!("could not run osascript to register {path}: {e}"))?;
    match out.stdout.trim() {
        "0" if out.success() => Ok(()),
        status => Err(format!(
            "registering {path} failed: status {status:?}, exit {:?}, {}",
            out.code,
            out.stderr.trim()
        )),
    }
}

/// Before a hook does anything: registers the bundle unless this copy of it
/// already was. `None` means nothing ran.
///
/// A failure is written down with its time, and the next attempt waits
/// [`RETRY_AFTER_MS`]: `pane.focused` alone can fire on every pane switch,
/// and each attempt costs `osascript`. The hook still posts after a
/// failure. Most Macs accept the app from its first run anyway, and where
/// they don't, the next registration that works puts it right.
///
/// A bundle whose files can't be read is skipped without a note, on every
/// hook; `doctor` and `--cleanup` report it.
pub fn ensure<R: Runner>(
    state: &StateDir,
    runner: &R,
    bundle: &Path,
    now_ms: u64,
) -> Option<Result<(), String>> {
    let key = marker_key(bundle).ok()?;
    let marker = fs::read_to_string(state.registered_path()).unwrap_or_default();
    if marker == key {
        return None;
    }
    let failed_at = marker
        .strip_prefix(FAILED)
        .and_then(|ms| ms.trim().parse::<u64>().ok());
    if failed_at.is_some_and(|at| now_ms.saturating_sub(at) < RETRY_AFTER_MS) {
        return None;
    }
    Some(register_and_mark(state, runner, bundle, &key, now_ms))
}

/// Registers whatever the marker says. Launch Services can lose an entry
/// between installs, and this runs at every server start.
pub fn always<R: Runner>(
    state: &StateDir,
    runner: &R,
    bundle: &Path,
    now_ms: u64,
) -> Result<(), String> {
    let key = marker_key(bundle).map_err(|e| unreadable(bundle, e))?;
    register_and_mark(state, runner, bundle, &key, now_ms)
}

/// How long after a failed registration the hooks leave it alone.
pub const RETRY_AFTER_MS: u64 = 30_000;

/// What the marker holds after a failure, followed by the time in ms. A key
/// starts with a number, so the two can't be mistaken for each other.
const FAILED: &str = "failed ";

fn register_and_mark<R: Runner>(
    state: &StateDir,
    runner: &R,
    bundle: &Path,
    key: &str,
    now_ms: u64,
) -> Result<(), String> {
    let path = state.registered_path();
    if let Err(e) = register(runner, bundle) {
        // Best effort: if this can't be written, neither could a key, and
        // every hook retries, which is the worst case with or without it.
        let _ = state::write_atomic(&path, format!("{FAILED}{now_ms}\n").as_bytes());
        return Err(e);
    }
    state::write_atomic(&path, key.as_bytes()).map_err(|e| {
        format!(
            "registered {} with Launch Services, but could not write {}: {e}",
            bundle.display(),
            path.display()
        )
    })
}

/// The line a hook logs for a registration that ran.
pub fn note(bundle: &Path, result: Result<(), String>) -> String {
    match result {
        Ok(()) => format!("registered {} with Launch Services", bundle.display()),
        Err(e) => e,
    }
}

/// The bundle's path and when its `Info.plist` and its binary last changed.
/// An install checks every file out afresh; a `git pull` in a linked
/// checkout only rewrites what changed, which may be the binary alone.
fn marker_key(bundle: &Path) -> io::Result<String> {
    let plist = modified_ns(&bundle.join("Contents/Info.plist"))?;
    let binary = modified_ns(&notifier::binary_in(bundle))?;
    Ok(format!("{plist} {binary} {}\n", bundle.display()))
}

fn modified_ns(path: &Path) -> io::Result<u128> {
    let modified = fs::metadata(path)?.modified()?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0))
}

/// A bundle without these files is broken, and macOS won't run it as an
/// app anyway, so there's nothing worth registering.
fn unreadable(bundle: &Path, e: io::Error) -> String {
    format!(
        "not registering {}: can't read its Info.plist or binary: {e}",
        bundle.display()
    )
}
