//! Registering the notifier with Launch Services, with `osascript` replaced
//! by its recorded answers.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use herdr_nudge::notifier::{binary_in, bundle_path};
use herdr_nudge::register::{self, RETRY_AFTER_MS, SCRIPT};
use herdr_nudge::state::StateDir;
use support::{Recorded, Replay, scratch_dir};

const NOW: u64 = 1_790_000_000_000;

/// A plugin folder with a bundle that has an `Info.plist` and a binary, and
/// an empty state directory.
fn setup(test_name: &str) -> (StateDir, PathBuf) {
    let dir = scratch_dir(test_name);
    let bundle = bundle_path(&dir.join("plugin"));
    fs::create_dir_all(binary_in(&bundle).parent().unwrap()).unwrap();
    fs::write(bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();
    fs::write(binary_in(&bundle), b"#!/bin/sh\n").unwrap();
    (StateDir::new(dir.join("state")), bundle)
}

fn argv(bundle: &Path) -> Vec<String> {
    ["/usr/bin/osascript", "-l", "JavaScript", "-e", SCRIPT]
        .into_iter()
        .map(str::to_owned)
        .chain([bundle.display().to_string()])
        .collect()
}

/// `osascript` answering for `bundle` the way it answered in `fixture`.
fn osascript(fixture: &str, bundle: &Path) -> Replay {
    let recorded = Recorded::sys(fixture);
    assert_eq!(
        recorded.argv[..5],
        ["osascript", "-l", "JavaScript", "-e", SCRIPT],
        "sys/{fixture}: captured with a different script"
    );
    let args: Vec<&str> = recorded.argv[1..5].iter().map(String::as_str).collect();
    let path = bundle.to_str().unwrap();
    Replay::answering("osascript", &[args.as_slice(), &[path]].concat(), &recorded)
}

fn touch(path: &Path) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(60))
        .unwrap();
}

#[test]
fn the_first_hook_registers_and_the_next_one_only_reads_the_marker() {
    let (state, bundle) = setup("register_once");
    let runner = osascript("osascript-register-bundle", &bundle);

    assert_eq!(
        register::ensure(&state, &runner, &bundle, NOW),
        Some(Ok(())),
        "first ensure"
    );
    assert_eq!(
        *runner.calls.borrow(),
        vec![argv(&bundle)],
        "osascript argv"
    );
    assert_eq!(
        register::ensure(&state, &runner, &bundle, NOW + 1),
        None,
        "second ensure"
    );
    assert_eq!(runner.call_count(), 1, "the second ensure ran osascript");
}

/// An install checks the whole bundle out again; a `git pull` in a linked
/// checkout may change only the binary.
#[test]
fn a_new_date_on_info_plist_or_the_binary_registers_again() {
    let (state, bundle) = setup("register_after_update");
    let runner = osascript("osascript-register-bundle", &bundle);
    register::ensure(&state, &runner, &bundle, NOW);

    touch(&bundle.join("Contents/Info.plist"));
    assert_eq!(
        register::ensure(&state, &runner, &bundle, NOW + 1),
        Some(Ok(())),
        "ensure after Info.plist's date moved"
    );
    touch(&binary_in(&bundle));
    assert_eq!(
        register::ensure(&state, &runner, &bundle, NOW + 2),
        Some(Ok(())),
        "ensure after the binary's date moved"
    );
    assert_eq!(runner.call_count(), 3, "osascript runs");
}

#[test]
fn a_moved_bundle_registers_again() {
    let (state, bundle) = setup("register_moved");
    let runner = osascript("osascript-register-bundle", &bundle);
    register::ensure(&state, &runner, &bundle, NOW);

    let moved = bundle.parent().unwrap().join("Moved.app");
    fs::rename(&bundle, &moved).unwrap();
    let runner = osascript("osascript-register-bundle", &moved);

    assert_eq!(
        register::ensure(&state, &runner, &moved, NOW + 1),
        Some(Ok(())),
        "ensure at the new path"
    );
}

/// `osascript` exits 0 here too; only the printed status says it failed.
/// The hooks leave it alone for a while, then try again.
#[test]
fn a_failed_registration_is_retried_after_the_wait() {
    let (state, bundle) = setup("register_fails");
    let runner = osascript("osascript-register-missing", &bundle);

    let Some(Err(e)) = register::ensure(&state, &runner, &bundle, NOW) else {
        panic!("sys/osascript-register-missing should read as a failure");
    };
    assert!(e.contains("\"-43\""), "status in the message: {e}");
    assert_eq!(
        register::ensure(&state, &runner, &bundle, NOW + RETRY_AFTER_MS - 1),
        None,
        "ensure within the wait"
    );
    assert_eq!(runner.call_count(), 1, "osascript runs within the wait");
    assert!(
        matches!(
            register::ensure(&state, &runner, &bundle, NOW + RETRY_AFTER_MS),
            Some(Err(_))
        ),
        "ensure after the wait should try again"
    );
    assert_eq!(runner.call_count(), 2, "osascript runs after the wait");
}

#[test]
fn osascript_that_wont_run_is_reported() {
    let (state, bundle) = setup("register_no_osascript");
    let runner = Replay::default();
    let Some(Err(e)) = register::ensure(&state, &runner, &bundle, NOW) else {
        panic!("no recording should read as a failure");
    };
    assert!(e.contains("could not run osascript"), "{e}");
}

/// Registered, but the marker can't be written: the note says both.
#[test]
fn an_unwritable_marker_is_not_reported_as_a_failed_registration() {
    let (_, bundle) = setup("register_unwritable");
    let blocker = scratch_dir("register_unwritable_state").join("not-a-dir");
    fs::write(&blocker, b"").unwrap();
    let state = StateDir::new(&blocker);
    let runner = osascript("osascript-register-bundle", &bundle);

    let Some(Err(e)) = register::ensure(&state, &runner, &bundle, NOW) else {
        panic!("a marker that can't be written should be reported");
    };
    assert!(
        e.starts_with("registered ") && e.contains("could not write"),
        "{e}"
    );
}

#[test]
fn always_registers_even_when_the_marker_matches_or_a_failure_is_recent() {
    let (state, bundle) = setup("register_always");
    let runner = osascript("osascript-register-bundle", &bundle);
    register::ensure(&state, &runner, &bundle, NOW);
    assert_eq!(register::always(&state, &runner, &bundle, NOW + 1), Ok(()));

    let failing = osascript("osascript-register-missing", &bundle);
    touch(&binary_in(&bundle));
    assert!(register::ensure(&state, &failing, &bundle, NOW + 2).is_some());
    assert!(register::always(&state, &failing, &bundle, NOW + 3).is_err());
    assert_eq!(
        runner.call_count() + failing.call_count(),
        4,
        "osascript runs"
    );
}

/// Nothing to key the marker on, and nothing macOS would run as an app.
/// The hooks skip it quietly; `doctor` and `--cleanup` say why.
#[test]
fn a_bundle_without_info_plist_is_not_registered() {
    let (state, bundle) = setup("register_no_plist");
    fs::remove_file(bundle.join("Contents/Info.plist")).unwrap();
    let runner = Replay::default();

    assert_eq!(register::ensure(&state, &runner, &bundle, NOW), None);
    let Err(e) = register::always(&state, &runner, &bundle, NOW) else {
        panic!("a missing Info.plist should be reported");
    };
    assert!(e.contains("Info.plist"), "{e}");
    assert_eq!(runner.call_count(), 0, "ran osascript");
}
