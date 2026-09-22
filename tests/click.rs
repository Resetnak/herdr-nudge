//! What a clicked notification does with its job file.

mod support;

use std::path::PathBuf;

use herdr_nudge::classify::{Classification, ClassifySignal, PaneKind};
use herdr_nudge::cli::JobId;
use herdr_nudge::click::{self, Outcome};
use herdr_nudge::event::AgentStatus;
use herdr_nudge::state::{Job, Loaded, PaneRecord, StateDir, VERSION};
use support::{Spy, scratch_dir};

fn job_id() -> JobId {
    JobId::parse("0123456789abcdef").expect("16 hex")
}

fn a_job(id: &JobId, expires_at_ms: u64) -> Job {
    Job {
        version: VERSION,
        id: id.to_string(),
        pane_id: "w1:p1".to_owned(),
        workspace_id: "w1".to_owned(),
        agent_label: Some("claude".to_owned()),
        kind: PaneKind::Agent,
        status: AgentStatus::Blocked,
        group: "herdr-nudge-w1:p1".to_owned(),
        bundle_id: Some("com.mitchellh.ghostty".to_owned()),
        socket_path: PathBuf::from("/tmp/herdr.sock"),
        notifier_path: PathBuf::from(
            "/plugin/vendor/HerdrNudge.app/Contents/MacOS/terminal-notifier",
        ),
        created_at_ms: 1_000,
        expires_at_ms,
        repeat_after_ms: 0,
    }
}

fn state_for(test_name: &str) -> StateDir {
    StateDir::new(scratch_dir(test_name).join("state"))
}

/// A notification can sit in Notification Center long after its job is gone.
#[test]
fn clicking_with_no_job_does_nothing() {
    let state = state_for("click_no_job");
    let spy = Spy::default();
    let (outcome, notes) = click::run(&state, &spy, &job_id(), 2_000);

    assert_eq!(outcome, Outcome::NoJob, "no job file");
    assert!(notes.is_empty(), "nothing worth logging: {notes:?}");
    assert!(
        spy.spawns.borrow().is_empty(),
        "nothing should have been run"
    );
}

#[test]
fn clicking_a_live_job_clears_it() {
    let state = state_for("click_live");
    let id = job_id();
    let job = a_job(&id, 9_000);
    state.save_job(&job).expect("save job");

    let mut record = PaneRecord::new(
        "w1:p1",
        Some("claude"),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::Catalogue,
        },
        1_000,
    );
    record.live_job = Some(id.to_string());
    state.save_pane_record(&record).expect("save record");

    let spy = Spy::default();
    let (outcome, notes) = click::run(&state, &spy, &id, 2_000);

    assert_eq!(
        outcome,
        Outcome::Cleared {
            pane_id: "w1:p1".to_owned()
        },
        "notes: {notes:?}"
    );
    assert_eq!(
        Spy::arg_after(&spy.only(), "-remove").as_deref(),
        Some("herdr-nudge-w1:p1"),
        "the group should be withdrawn"
    );
    assert_eq!(
        spy.only()[0],
        job.notifier_path.display().to_string(),
        "the notifier path should come from the job file"
    );
    assert!(
        matches!(state.job(&id), Ok(Loaded::Missing)),
        "the job file should be gone"
    );

    let Ok(Loaded::Found(after)) = state.pane_record("w1:p1") else {
        panic!("the pane record should still be there");
    };
    assert_eq!(after.live_job, None, "live_job should be cleared");
    assert_eq!(
        after.kind,
        PaneKind::Agent,
        "the classification should survive a click"
    );
}

#[test]
fn clicking_an_expired_job_clears_it_without_focusing() {
    let state = state_for("click_expired");
    let id = job_id();
    state.save_job(&a_job(&id, 1_500)).expect("save job");

    let spy = Spy::default();
    let (outcome, _) = click::run(&state, &spy, &id, 1_500);

    assert_eq!(
        outcome,
        Outcome::Expired,
        "expiry is inclusive of the deadline"
    );
    assert!(
        matches!(state.job(&id), Ok(Loaded::Missing)),
        "an expired job should be tidied away"
    );
}

/// Another pane's notification must not be cleared by this one's click.
#[test]
fn a_click_leaves_another_panes_live_job_alone() {
    let state = state_for("click_other_pane");
    let id = job_id();
    state.save_job(&a_job(&id, 9_000)).expect("save job");

    let mut other = PaneRecord::new(
        "w1:p2",
        Some("make"),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Neither,
        },
        1_000,
    );
    other.live_job = Some("fedcba9876543210".to_owned());
    state.save_pane_record(&other).expect("save record");

    let spy = Spy::default();
    click::run(&state, &spy, &id, 2_000);

    let Ok(Loaded::Found(after)) = state.pane_record("w1:p2") else {
        panic!("the other pane's record should still be there");
    };
    assert_eq!(
        after.live_job.as_deref(),
        Some("fedcba9876543210"),
        "the other pane's notification should be untouched"
    );
}

/// The click has no HERDR_* environment, so it has to work out where the
/// state directory is.
#[test]
fn the_state_directory_is_found_without_any_herdr_environment() {
    let from_herdr = StateDir::locate(|name| match name {
        "HERDR_PLUGIN_STATE_DIR" => Some("/given/by/herdr".to_owned()),
        "HOME" => Some("/Users/dev".to_owned()),
        _ => None,
    });
    assert_eq!(
        from_herdr.map(|s| s.root),
        Some(PathBuf::from("/given/by/herdr")),
        "the variable wins when Herdr set it"
    );

    let from_home = StateDir::locate(|name| match name {
        "HOME" => Some("/Users/dev".to_owned()),
        _ => None,
    });
    assert_eq!(
        from_home.map(|s| s.root),
        Some(PathBuf::from(
            "/Users/dev/.local/state/herdr/plugins/herdr-nudge"
        )),
        "the path Herdr 0.9.0 uses, as captured in tests/fixtures/events/"
    );

    let nothing = StateDir::locate(|_| None);
    assert!(nothing.is_none(), "with no HOME there is nowhere to look");
}

/// An empty variable is not a path.
#[test]
fn an_empty_state_dir_variable_falls_through_to_home() {
    let located = StateDir::locate(|name| match name {
        "HERDR_PLUGIN_STATE_DIR" => Some(String::new()),
        "HOME" => Some("/Users/dev".to_owned()),
        _ => None,
    });
    assert_eq!(
        located.map(|s| s.root),
        Some(PathBuf::from(
            "/Users/dev/.local/state/herdr/plugins/herdr-nudge"
        )),
        "an empty HERDR_PLUGIN_STATE_DIR should not be used as a path"
    );
}

/// A failure to withdraw the notification must not leave the job behind, or
/// the next click would try again forever.
#[test]
fn the_job_is_deleted_even_if_the_notifier_will_not_start() {
    let state = state_for("click_notifier_fails");
    let id = job_id();
    state.save_job(&a_job(&id, 9_000)).expect("save job");

    let spy = Spy::failing();
    let (outcome, notes) = click::run(&state, &spy, &id, 2_000);

    assert!(
        matches!(outcome, Outcome::Cleared { .. }),
        "outcome: {outcome:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("could not remove group")),
        "the failure should be logged: {notes:?}"
    );
    assert!(
        matches!(state.job(&id), Ok(Loaded::Missing)),
        "the job file should still be deleted"
    );
}
