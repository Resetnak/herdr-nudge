//! State files: atomic writes, and recovery from files we can't use.

mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use herdr_nudge::classify::{Classification, ClassifySignal, PaneKind};
use herdr_nudge::state::{
    AgentsCache, FOCUS_ORIGIN_TTL_MS, FocusOrigin, Loaded, PaneRecord, StateDir, TerminalMemory,
    VERSION, write_atomic,
};
use support::scratch_dir;

fn leftovers(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn terminal_memory_round_trips() {
    let state = StateDir::new(scratch_dir("terminal_memory_round_trips"));
    let mut memory = TerminalMemory::default();
    memory.set("w1", "com.mitchellh.ghostty", 1_000);
    state.save_terminal_memory(&memory).unwrap();

    match state.terminal_memory().unwrap() {
        Loaded::Found(read) => assert_eq!(read, memory),
        other => panic!("terminal-memory.json: expected Found, got {other:?}"),
    }
}

#[test]
fn a_missing_file_is_missing_not_an_error() {
    let state = StateDir::new(scratch_dir("a_missing_file_is_missing_not_an_error"));
    assert!(matches!(state.terminal_memory().unwrap(), Loaded::Missing));
    assert!(matches!(state.focus_origin().unwrap(), Loaded::Missing));
}

#[test]
fn a_write_leaves_no_temp_file_and_is_private() {
    let dir = scratch_dir("a_write_leaves_no_temp_file_and_is_private");
    let path = dir.join("f.json");
    write_atomic(&path, b"one").unwrap();
    write_atomic(&path, b"two").unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"two");
    assert_eq!(leftovers(&dir), ["f.json"]);
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "f.json mode");
}

#[test]
fn a_write_creates_missing_parent_directories() {
    let dir = scratch_dir("a_write_creates_missing_parent_directories");
    let path = dir.join("jobs/nested/f.json");
    write_atomic(&path, b"x").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"x");
}

/// Renaming a file over a non-empty directory fails, which makes the write
/// fail after its temp file already exists.
#[test]
fn a_failed_write_removes_its_temp_file() {
    let dir = scratch_dir("a_failed_write_removes_its_temp_file");
    let path = dir.join("f.json");
    write_atomic(&path, b"old").unwrap();

    let blocker = dir.join("blocker");
    fs::create_dir_all(blocker.join("inside")).unwrap();
    assert!(write_atomic(&blocker, b"new").is_err());

    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(
        leftovers(&dir),
        ["blocker", "f.json"],
        "temp file left behind"
    );
}

/// Readers running alongside a writer must only ever see whole files.
#[test]
fn a_reader_never_sees_a_half_written_file() {
    let dir = scratch_dir("a_reader_never_sees_a_half_written_file");
    let path = dir.join("f.json");
    let small = vec![b'a'; 10];
    let large = vec![b'b'; 1 << 20];
    write_atomic(&path, &small).unwrap();

    let writer = {
        let (path, small, large) = (path.clone(), small.clone(), large.clone());
        std::thread::spawn(move || {
            for i in 0..50 {
                write_atomic(&path, if i % 2 == 0 { &large } else { &small }).unwrap();
            }
        })
    };
    while !writer.is_finished() {
        let read = fs::read(&path).unwrap();
        assert!(read == small || read == large, "read {} bytes", read.len());
    }
    writer.join().unwrap();
}

#[test]
fn a_write_replaces_a_symlink_instead_of_following_it() {
    let dir = scratch_dir("a_write_replaces_a_symlink_instead_of_following_it");
    let target = dir.join("elsewhere.txt");
    fs::write(&target, b"untouched").unwrap();
    let path = dir.join("f.json");
    symlink(&target, &path).unwrap();

    write_atomic(&path, b"ours").unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"untouched");
    assert!(
        !fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&path).unwrap(), b"ours");
}

fn assert_recovered(state: &StateDir, contents: &[u8], test: &str) {
    let path = state.terminal_memory_path();
    fs::write(&path, contents).unwrap();

    let Loaded::Recovered(recovered) = state.terminal_memory().unwrap() else {
        panic!("{test}: expected Recovered");
    };
    let aside = recovered.moved_to.expect("moved aside, not deleted");
    assert_eq!(
        fs::read(&aside).unwrap(),
        contents,
        "{test}: set-aside copy"
    );
    assert!(!path.exists(), "{test}: original still in place");

    // Recovery means the next save and load work normally.
    let mut memory = TerminalMemory::default();
    memory.set("w1", "com.mitchellh.ghostty", 1);
    state.save_terminal_memory(&memory).unwrap();
    assert!(matches!(state.terminal_memory().unwrap(), Loaded::Found(m) if m == memory));
}

#[test]
fn a_corrupt_file_is_moved_aside_and_replaced() {
    for (name, contents) in [
        ("truncated", &b"{\"version\":1,\"workspa"[..]),
        ("empty", b""),
        ("not_json", b"\x00\xff garbage"),
        ("wrong_shape", b"{\"version\":1,\"workspaces\":[1,2]}"),
    ] {
        let state = StateDir::new(scratch_dir(&format!("corrupt_{name}")));
        assert_recovered(&state, contents, name);
    }
}

#[test]
fn a_file_from_another_version_is_moved_aside() {
    let state = StateDir::new(scratch_dir("a_file_from_another_version_is_moved_aside"));
    let newer = format!("{{\"version\":{},\"workspaces\":{{}}}}", VERSION + 1);
    assert_recovered(&state, newer.as_bytes(), "newer version");
}

#[test]
fn a_symlinked_state_file_is_not_followed() {
    let dir = scratch_dir("a_symlinked_state_file_is_not_followed");
    let target = dir.join("elsewhere.json");
    fs::write(&target, b"{\"version\":1,\"workspaces\":{}}").unwrap();
    let state = StateDir::new(&dir);
    symlink(&target, state.terminal_memory_path()).unwrap();

    let Loaded::Recovered(recovered) = state.terminal_memory().unwrap() else {
        panic!("terminal-memory.json symlink: expected Recovered");
    };
    assert_eq!(recovered.reason, "not a regular file");
    assert!(target.exists(), "the symlink's target was touched");
}

#[test]
fn a_recovered_file_reads_as_the_default() {
    let state = StateDir::new(scratch_dir("a_recovered_file_reads_as_the_default"));
    fs::write(state.focus_origin_path(), b"nope").unwrap();
    assert_eq!(
        state.focus_origin().unwrap().into_value(),
        FocusOrigin::default()
    );
}

#[test]
fn a_focus_mark_lasts_fifteen_seconds() {
    let mut origin = FocusOrigin::default();
    origin.mark("w1", 100_000);

    assert!(origin.is_recent("w1", 100_000));
    assert!(origin.is_recent("w1", 100_000 + FOCUS_ORIGIN_TTL_MS - 1));
    assert!(!origin.is_recent("w1", 100_000 + FOCUS_ORIGIN_TTL_MS));
    assert!(!origin.is_recent("w2", 100_000), "w2 was never marked");
    // The clock went back: still treat it as recent.
    assert!(origin.is_recent("w1", 50_000));
}

#[test]
fn marking_drops_expired_entries() {
    let mut origin = FocusOrigin::default();
    origin.mark("old", 0);
    origin.mark("w1", FOCUS_ORIGIN_TTL_MS + 1);
    assert_eq!(origin.workspaces.keys().collect::<Vec<_>>(), ["w1"]);
}

/// The click can read a state file while an event hook is reading the same
/// one. Both find it corrupt, only one can move it aside, and the loser
/// must still come back with something usable.
#[test]
fn two_readers_of_one_corrupt_file_both_recover() {
    let state = StateDir::new(scratch_dir("two_readers_of_one_corrupt_file"));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            std::thread::spawn(move || {
                for _ in 0..50 {
                    fs::write(state.terminal_memory_path(), b"not json").unwrap();
                    state
                        .terminal_memory()
                        .expect("read of a corrupt file failed");
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn the_agents_cache_round_trips() {
    let state = StateDir::new(scratch_dir("the_agents_cache_round_trips"));
    let cache = AgentsCache::new(["claude", "codex"].map(String::from), 1_000);
    state.save_agents_cache(&cache).unwrap();

    match state.agents_cache().unwrap() {
        Loaded::Found(read) => {
            assert_eq!(read, cache);
            assert_eq!(read.labels().collect::<Vec<_>>(), ["claude", "codex"]);
        }
        other => panic!("agents-cache.json: expected Found, got {other:?}"),
    }
}

fn record(pane_id: &str, agent: &str, kind: PaneKind) -> PaneRecord {
    PaneRecord::new(
        pane_id,
        Some(agent),
        Classification {
            kind,
            signal: ClassifySignal::Catalogue,
        },
        1_000,
    )
}

#[test]
fn a_pane_record_round_trips_and_is_forgotten() {
    let state = StateDir::new(scratch_dir("a_pane_record_round_trips_and_is_forgotten"));
    let written = record("w3:p1", "claude", PaneKind::Agent);
    state.save_pane_record(&written).unwrap();

    match state.pane_record("w3:p1").unwrap() {
        Loaded::Found(read) => assert_eq!(read, written),
        other => panic!("panes/w3:p1: expected Found, got {other:?}"),
    }
    assert!(matches!(
        state.pane_record("w3:p2").unwrap(),
        Loaded::Missing
    ));

    state.forget_pane("w3:p1").unwrap();
    assert!(matches!(
        state.pane_record("w3:p1").unwrap(),
        Loaded::Missing
    ));
    // Forgetting a pane twice is what a close after a release looks like.
    state.forget_pane("w3:p1").unwrap();
}

#[test]
fn a_pane_id_cannot_name_a_file_outside_the_panes_directory() {
    let state = StateDir::new(scratch_dir(
        "a_pane_id_cannot_name_a_file_outside_the_panes_directory",
    ));
    let escaping = "../../w3:p1";
    state
        .save_pane_record(&record(escaping, "claude", PaneKind::Agent))
        .unwrap();

    let path = state.pane_record_path(escaping);
    assert_eq!(path.parent().unwrap(), state.panes_dir());
    assert_eq!(leftovers(&state.panes_dir()).len(), 1);
    match state.pane_record(escaping).unwrap() {
        Loaded::Found(read) => assert_eq!(read.pane_id, escaping),
        other => panic!("{}: expected Found, got {other:?}", path.display()),
    }
}
