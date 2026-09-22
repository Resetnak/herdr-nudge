//! One event, end to end, with Herdr and the notifier replaced by recordings.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use herdr_nudge::classify::{ClassifySignal, PaneKind};
use herdr_nudge::cli::JobId;
use herdr_nudge::config::Config;
use herdr_nudge::event::{AgentStatus, Envelope};
use herdr_nudge::handler::{self, Deps, Outcome};
use herdr_nudge::state::{AgentsCache, Loaded, StateDir, now_ms};
use support::{Fixture, Recorded, Replay, Spy, scratch_dir};

/// A state directory, a fake plugin with a fake notifier in it, and a `herdr`
/// that only answers what it was given.
struct Harness {
    state: StateDir,
    /// Must be named `herdr`: the replay matches a recording by the
    /// program's file name.
    herdr_bin: PathBuf,
    config: Config,
    runner: Replay,
    spy: Spy,
    plugin_root: PathBuf,
    notifier_bin: PathBuf,
    self_bin: PathBuf,
    socket: PathBuf,
    now_ms: u64,
}

impl Harness {
    fn new(test_name: &str, recordings: Vec<Recorded>) -> Harness {
        let dir = scratch_dir(test_name);
        let plugin_root = dir.join("plugin");
        let notifier_bin = herdr_nudge::notifier::binary_path(&plugin_root);
        fs::create_dir_all(notifier_bin.parent().expect("bundle dir")).expect("bundle dir");
        fs::write(&notifier_bin, b"#!/bin/sh\n").expect("fake notifier");
        // Absolute, because the click command refuses anything else.
        let self_bin = plugin_root.join("bin/herdr-nudge");
        fs::create_dir_all(self_bin.parent().expect("bin dir")).expect("bin dir");
        fs::write(&self_bin, b"#!/bin/sh\n").expect("fake binary");

        Harness {
            state: StateDir::new(dir.join("state")),
            herdr_bin: dir.join("herdr"),
            config: Config::default(),
            runner: Replay::new(recordings),
            spy: Spy::default(),
            plugin_root,
            notifier_bin,
            self_bin,
            socket: dir.join("herdr.sock"),
            now_ms: now_ms(),
        }
    }

    /// `herdr pane get <pane>` answered from a recording, whatever pane the
    /// recording was captured against.
    fn answering(test_name: &str, pane_id: &str, recording: &str) -> Harness {
        let mut harness = Harness::new(test_name, Vec::new());
        harness.runner = Replay::answering(
            "herdr",
            &["pane", "get", pane_id],
            &Recorded::cli(recording),
        );
        harness
    }

    fn deps(&self) -> Deps<'_, Replay, Spy> {
        Deps {
            config: &self.config,
            state: &self.state,
            herdr_bin: &self.herdr_bin,
            socket_path: &self.socket,
            notifier_bin: &self.notifier_bin,
            self_bin: &self.self_bin,
            plugin_root: &self.plugin_root,
            runner: &self.runner,
            spawner: &self.spy,
            now_ms: self.now_ms,
            pid: 4242,
        }
    }

    fn handle(&self, fixture: &str) -> Outcome {
        self.handle_envelope(&Fixture::load(fixture).envelope())
    }

    fn handle_envelope(&self, envelope: &Envelope) -> Outcome {
        handler::handle(&self.deps(), envelope, None).outcome
    }

    fn remember_agents(&self, labels: &[&str]) {
        let cache = AgentsCache::new(labels.iter().map(|l| l.to_string()), self.now_ms);
        self.state.save_agents_cache(&cache).expect("save cache");
    }
}

/// The cheap gate: a status no trigger set names must not cost a subprocess.
#[test]
fn a_working_event_asks_herdr_nothing() {
    let harness = Harness::new("working_asks_nothing", Vec::new());
    let outcome = harness.handle("agent/working");
    assert_eq!(
        outcome,
        Outcome::StatusNotWatched(AgentStatus::Working),
        "agent/working should stop before any query"
    );
    assert_eq!(
        harness.runner.call_count(),
        0,
        "agent/working ran a subprocess"
    );
    assert!(
        harness.spy.spawns.borrow().is_empty(),
        "agent/working started the notifier"
    );
}

#[test]
fn an_unknown_status_is_watched_by_nobody() {
    let harness = Harness::new("unknown_status", Vec::new());
    assert_eq!(
        harness.handle("agent/status-unknown-no-agent-field"),
        Outcome::StatusNotWatched(AgentStatus::Unknown),
        "an unknown status is not a trigger"
    );
    assert_eq!(
        harness.runner.call_count(),
        0,
        "unknown status queried Herdr"
    );
}

/// A pane Herdr detects by itself is an agent, and `blocked` notifies.
#[test]
fn a_blocked_agent_posts_a_notification() {
    let harness = Harness::answering("blocked_posts", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);

    let Outcome::Posted(posted) = harness.handle("agent/blocked") else {
        panic!(
            "agent/blocked did not post: {:?}",
            harness.handle("agent/blocked")
        );
    };
    assert_eq!(posted.kind, PaneKind::Agent, "agent/blocked pane kind");
    assert_eq!(
        posted.signal,
        ClassifySignal::Catalogue,
        "claude is in the remembered manifests"
    );
    assert_eq!(posted.group, "herdr-nudge-w1:p1", "agent/blocked group");
    assert_eq!(posted.title, "Claude · blocked", "agent/blocked title");

    let argv = harness.spy.only();
    assert_eq!(
        argv[0],
        harness.notifier_bin.display().to_string(),
        "the bundled notifier was started"
    );
    assert_eq!(
        Spy::arg_after(&argv, "-execute"),
        Some(format!(
            "'{}' --click {}",
            harness.self_bin.display(),
            posted.job_id
        )),
        "the -execute value"
    );
}

/// Everything the click needs has to be on disk before the banner is up,
/// because the click gets no environment at all.
#[test]
fn the_job_file_carries_what_the_click_cannot_look_up() {
    let mut harness = Harness::answering("job_file", "w1:p1", "pane-get-unfocused");
    harness.config.default_terminal = Some("com.mitchellh.ghostty".to_owned());
    harness.remember_agents(&["claude"]);

    let Outcome::Posted(posted) = harness.handle("agent/blocked") else {
        panic!("agent/blocked did not post");
    };
    let Ok(Loaded::Found(job)) = harness.state.job(&posted.job_id) else {
        panic!("no job file for {}", posted.job_id);
    };

    assert_eq!(job.pane_id, "w1:p1", "job pane_id");
    assert_eq!(job.workspace_id, "w1", "job workspace_id");
    assert_eq!(
        job.agent_label.as_deref(),
        Some("claude"),
        "job agent_label"
    );
    assert_eq!(job.kind, PaneKind::Agent, "job kind");
    assert_eq!(job.status, AgentStatus::Blocked, "job status");
    assert_eq!(
        job.bundle_id.as_deref(),
        Some("com.mitchellh.ghostty"),
        "job bundle_id, from default_terminal"
    );
    assert_eq!(job.socket_path, harness.socket, "job socket_path");
    assert_eq!(job.notifier_path, harness.notifier_bin, "job notifier_path");
    assert!(
        job.expires_at_ms > job.created_at_ms,
        "job expiry {} is not after creation {}",
        job.expires_at_ms,
        job.created_at_ms
    );
}

/// Reporting metadata alone emits a status event carrying the unchanged
/// status, so the same status arrives more than once.
#[test]
fn the_same_status_twice_posts_once() {
    let harness = Harness::answering("dedup", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);

    let first = harness.handle("agent/blocked");
    assert!(matches!(first, Outcome::Posted(_)), "first: {first:?}");

    let second = harness.handle("agent/blocked");
    let Outcome::AlreadyShowing(job_id) = &second else {
        panic!("second agent/blocked should have been suppressed: {second:?}");
    };
    if let Outcome::Posted(posted) = first {
        assert_eq!(
            *job_id,
            posted.job_id.to_string(),
            "the live job should be the one already posted"
        );
    }
    assert_eq!(
        harness.spy.spawns.borrow().len(),
        1,
        "the notifier should have run once"
    );
}

/// A different status for the same pane is a different thing to say.
#[test]
fn blocked_then_done_posts_twice() {
    let harness = Harness::answering("blocked_then_done", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);

    assert!(matches!(
        harness.handle("agent/blocked"),
        Outcome::Posted(_)
    ));
    let second = harness.handle("agent/done");
    assert!(
        matches!(second, Outcome::Posted(_)),
        "agent/done after blocked: {second:?}"
    );
    assert_eq!(
        harness.spy.spawns.borrow().len(),
        2,
        "both statuses should post"
    );
}

/// Focused pane plus the bound terminal in front means the user is looking
/// at it.
#[test]
fn a_pane_the_user_is_watching_stays_quiet() {
    let mut harness = Harness::new("watching", Vec::new());
    let mut recordings = vec![
        Recorded::sys("lsappinfo-front"),
        Recorded::sys("lsappinfo-bundleid-ghostty"),
    ];
    let mut pane_get = Recorded::cli("pane-get-focused");
    pane_get.argv = ["herdr", "pane", "get", "w1:p1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    recordings.push(pane_get);
    harness.runner = Replay::new(recordings);
    harness.config.default_terminal = Some("com.mitchellh.ghostty".to_owned());
    harness.remember_agents(&["claude"]);

    assert_eq!(
        harness.handle("agent/blocked"),
        Outcome::Watching,
        "focused pane in the frontmost terminal should not notify"
    );
    assert!(
        harness.spy.spawns.borrow().is_empty(),
        "nothing should have been posted"
    );
}

/// A pane the user is on, but some other app is in front: notify.
#[test]
fn a_focused_pane_behind_another_app_still_notifies() {
    let mut harness = Harness::new("focused_but_behind", Vec::new());
    let mut pane_get = Recorded::cli("pane-get-focused");
    pane_get.argv = ["herdr", "pane", "get", "w1:p1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut front = Recorded::sys("lsappinfo-bundleid-ghostty");
    front.stdout = "\"CFBundleIdentifier\"=\"com.apple.Safari\"\n".to_owned();
    harness.runner = Replay::new(vec![pane_get, Recorded::sys("lsappinfo-front"), front]);
    harness.config.default_terminal = Some("com.mitchellh.ghostty".to_owned());
    harness.remember_agents(&["claude"]);

    let outcome = harness.handle("agent/blocked");
    assert!(
        matches!(outcome, Outcome::Posted(_)),
        "a browser in front should not suppress: {outcome:?}"
    );
}

/// When the query fails we know neither whether the user is watching nor
/// what the pane is. A spurious notification beats a missed one.
#[test]
fn a_failed_pane_query_still_notifies() {
    let harness = Harness::new("query_fails", Vec::new());
    let outcome = harness.handle("shell/done-unwatched-failed");
    assert!(
        matches!(outcome, Outcome::Posted(_)),
        "an unanswered pane get should not silence us: {outcome:?}"
    );
}

#[test]
fn a_shell_command_is_classified_and_titled_as_one() {
    let harness = Harness::answering("shell_classified", "w3:p3", "pane-get-unfocused");
    harness.remember_agents(&["claude", "codex"]);

    let Outcome::Posted(posted) = harness.handle("shell/done-unwatched-failed") else {
        panic!("shell/done-unwatched-failed did not post");
    };
    assert_eq!(posted.kind, PaneKind::Shell, "make is not a known agent");
    assert_eq!(
        posted.signal,
        ClassifySignal::Neither,
        "no catalogue entry and no session"
    );
    assert_eq!(posted.title, "make · failed", "shell title");
}

#[test]
fn notify_on_failure_only_drops_a_command_that_worked() {
    let mut harness = Harness::answering("failure_only", "w3:p4", "pane-get-unfocused");
    harness.config.shell.notify_on_failure_only = true;

    assert_eq!(
        harness.handle("shell/done-unwatched-after-handover"),
        Outcome::NotAFailure,
        "state_labels[idle] is \"done\", not \"failed\""
    );
}

#[test]
fn notify_on_failure_only_keeps_a_command_that_failed() {
    let mut harness = Harness::answering("failure_only_keeps", "w3:p3", "pane-get-unfocused");
    harness.config.shell.notify_on_failure_only = true;

    let outcome = harness.handle("shell/done-unwatched-failed");
    assert!(
        matches!(outcome, Outcome::Posted(_)),
        "state_labels[idle] is \"failed\": {outcome:?}"
    );
}

#[test]
fn a_disabled_half_never_notifies() {
    let mut harness = Harness::answering("agents_disabled", "w1:p1", "pane-get-unfocused");
    harness.config.agents.enabled = false;
    harness.remember_agents(&["claude"]);

    // The union gate sees `blocked` is still wanted by the shell side, so the
    // query happens; classification then lands on the disabled half.
    assert_eq!(
        harness.handle("agent/blocked"),
        Outcome::StatusNotWatched(AgentStatus::Blocked),
        "with agents off, blocked is in neither enabled trigger set"
    );
}

#[test]
fn an_ignored_agent_label_is_dropped() {
    let mut harness = Harness::answering("ignored_agent", "w3:p3", "pane-get-unfocused");
    harness.config.shell.ignore_agents = vec!["make".to_owned()];

    assert_eq!(
        harness.handle("shell/done-unwatched-failed"),
        Outcome::IgnoredAgent("make".to_owned()),
        "make is in ignore_agents"
    );
}

#[test]
fn a_missing_notifier_is_reported_not_ignored() {
    let harness = Harness::answering("no_notifier", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);
    fs::remove_file(&harness.notifier_bin).expect("remove the fake notifier");

    assert_eq!(
        harness.handle("agent/blocked"),
        Outcome::NotifierMissing(harness.notifier_bin.clone()),
        "a missing bundle should say so"
    );
}

#[test]
fn an_expired_job_is_swept_and_its_banner_withdrawn() {
    let mut harness = Harness::answering("sweep", "w1:p1", "pane-get-unfocused");
    harness.config.notifications.clickable_secs = 0;
    harness.remember_agents(&["claude"]);

    let Outcome::Posted(posted) = harness.handle("agent/blocked") else {
        panic!("agent/blocked did not post");
    };
    harness.spy.spawns.borrow_mut().clear();

    let (swept, _) = handler::sweep_expired(&harness.state, &harness.spy, harness.now_ms + 1);
    assert_eq!(swept, vec![posted.job_id.clone()], "the expired job");
    assert_eq!(
        Spy::arg_after(&harness.spy.only(), "-remove").as_deref(),
        Some("herdr-nudge-w1:p1"),
        "the group should be withdrawn"
    );
    assert!(
        matches!(harness.state.job(&posted.job_id), Ok(Loaded::Missing)),
        "the job file should be gone"
    );
}

#[test]
fn a_live_job_is_left_alone_by_the_sweep() {
    let harness = Harness::answering("sweep_keeps", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);
    let Outcome::Posted(posted) = harness.handle("agent/blocked") else {
        panic!("agent/blocked did not post");
    };
    harness.spy.spawns.borrow_mut().clear();

    let (swept, _) = handler::sweep_expired(&harness.state, &harness.spy, harness.now_ms);
    assert!(
        swept.is_empty(),
        "swept a job that has not expired: {swept:?}"
    );
    assert!(
        matches!(harness.state.job(&posted.job_id), Ok(Loaded::Found(_))),
        "the job file should still be there"
    );
}

/// Events other than a status change do nothing yet.
#[test]
fn other_events_are_not_handled_yet() {
    let harness = Harness::new("other_events", Vec::new());
    for fixture in ["lifecycle/pane-closed", "focus/tab-focus-pane-focused"] {
        assert_eq!(
            harness.handle(fixture),
            Outcome::NotHandled,
            "{fixture} is not a status change, so nothing should happen"
        );
    }
}

/// No event should ever make the hook panic, whatever it carries.
#[test]
fn every_captured_event_is_handled_without_panicking() {
    let harness = Harness::new("all_fixtures", Vec::new());
    for fixture in Fixture::all() {
        let outcome = harness.handle_envelope(&fixture.envelope());
        assert!(
            !matches!(outcome, Outcome::Failed(_)),
            "{}: {outcome:?}",
            fixture.name
        );
    }
}

/// A label that would climb out of the icons directory is not used as a path.
#[test]
fn a_strange_agent_label_gets_no_logo() {
    let harness = Harness::answering("strange_label", "w1:p1", "pane-get-unfocused");
    let fixture = Fixture::load("agent/blocked");
    let json = fixture.event_json_with("agent", "../../../etc/passwd".into());
    let envelope = Envelope::parse(&json).expect("mutated fixture parses");
    harness.remember_agents(&["../../../etc/passwd"]);

    let outcome = harness.handle_envelope(&envelope);
    assert!(
        matches!(outcome, Outcome::Posted(_)),
        "should still post: {outcome:?}"
    );
    let argv = harness.spy.only();
    assert!(
        Spy::arg_after(&argv, "-contentImage").is_none(),
        "a label with path segments must not become an image path: {argv:?}"
    );
}

#[test]
fn job_ids_are_sixteen_hex_and_differ_between_events() {
    let mut seen = BTreeSet::new();
    // Same clock, same pid: only the counter separates these two.
    for ms in [1_700_000_000_000u64, 1_700_000_000_000, 1_700_000_000_001] {
        let id = herdr_nudge::state::new_job_id(ms, 4242);
        assert_eq!(id.as_str().len(), 16, "job id length: {id}");
        assert!(JobId::parse(id.as_str()).is_ok(), "job id shape: {id}");
        assert!(seen.insert(id.to_string()), "duplicate job id: {id}");
    }
}

/// The dedup window is there to swallow a repeat of the same status, which
/// arrives because reporting metadata alone emits one.
#[test]
fn a_repeat_within_the_window_is_suppressed() {
    let harness = Harness::answering("dedup_window", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);
    assert!(matches!(
        harness.handle("agent/blocked"),
        Outcome::Posted(_)
    ));

    let again = harness.handle("agent/blocked");
    assert!(
        matches!(again, Outcome::AlreadyShowing(_)),
        "a repeat straight away should be suppressed: {again:?}"
    );
}

/// Dismissing a banner tells us nothing, so the window has to let go long
/// before the notification stops being clickable — otherwise a swipe silences
/// the pane for the rest of the hour.
#[test]
fn a_repeat_after_the_window_posts_again_while_still_clickable() {
    let mut harness = Harness::answering("dedup_expires", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);

    let Outcome::Posted(first) = harness.handle("agent/blocked") else {
        panic!("the first agent/blocked did not post");
    };
    let Ok(Loaded::Found(job)) = harness.state.job(&first.job_id) else {
        panic!("no job file");
    };

    harness.now_ms = job.repeat_after_ms;
    assert!(
        !job.is_expired(harness.now_ms),
        "the notification should still be clickable at this point"
    );

    let again = harness.handle("agent/blocked");
    assert!(
        matches!(again, Outcome::Posted(_)),
        "once the window passes the same status should notify again: {again:?}"
    );
}

/// The window is much shorter than the clickable lifetime, which is the whole
/// point of it being a separate deadline.
#[test]
fn the_dedup_window_is_far_shorter_than_the_clickable_lifetime() {
    let harness = Harness::answering("dedup_shorter", "w1:p1", "pane-get-unfocused");
    harness.remember_agents(&["claude"]);
    let Outcome::Posted(posted) = harness.handle("agent/blocked") else {
        panic!("agent/blocked did not post");
    };
    let Ok(Loaded::Found(job)) = harness.state.job(&posted.job_id) else {
        panic!("no job file");
    };
    assert!(
        job.repeat_after_ms < job.expires_at_ms,
        "repeat_after_ms {} should be well before expires_at_ms {}",
        job.repeat_after_ms,
        job.expires_at_ms
    );
}

/// An expired job blocks nothing, however its window was set.
#[test]
fn an_expired_job_never_blocks_a_repeat() {
    let job_at = |repeat_after_ms, expires_at_ms| herdr_nudge::state::Job {
        version: herdr_nudge::state::VERSION,
        id: "0123456789abcdef".to_owned(),
        pane_id: "w1:p1".to_owned(),
        workspace_id: "w1".to_owned(),
        agent_label: None,
        kind: PaneKind::Agent,
        status: AgentStatus::Blocked,
        group: "g".to_owned(),
        bundle_id: None,
        socket_path: PathBuf::new(),
        notifier_path: PathBuf::new(),
        created_at_ms: 0,
        expires_at_ms,
        repeat_after_ms,
    };

    assert!(
        !job_at(9_000, 1_000).blocks_repeat(2_000),
        "an expired job should not block a repeat even mid-window"
    );
    assert!(
        job_at(9_000, 99_000).blocks_repeat(2_000),
        "a live job inside its window should block"
    );
    // A job written before the field existed reads as 0, which blocks
    // nothing — err towards notifying.
    assert!(
        !job_at(0, 99_000).blocks_repeat(2_000),
        "a job with no window should block nothing"
    );
}

/// A pane that hands over from an agent to a shell reporter has a new agent
/// identity, so the pane record carries no job id for the old one. The old
/// job still has to go: the group is per-pane, so a sweep of it would later
/// withdraw the notification the new job owns.
#[test]
fn a_handover_leaves_one_job_for_the_pane() {
    let mut harness = Harness::new("handover_one_job", Vec::new());
    let mut pane_get = Recorded::cli("pane-get-unfocused");
    pane_get.argv = ["herdr", "pane", "get", "w1:p1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    harness.runner = Replay::new(vec![pane_get]);
    harness.remember_agents(&["claude"]);

    let Outcome::Posted(agent_job) = harness.handle("agent/blocked") else {
        panic!("agent/blocked did not post");
    };

    // The same pane, now reporting as `make`: a different identity.
    harness.now_ms += 1_000;
    let shell = Fixture::load("shell/done-unwatched-failed");
    let json = shell.event_json_with("pane_id", "w1:p1".into());
    let envelope = Envelope::parse(&json).expect("retargeted fixture parses");
    let Outcome::Posted(shell_job) = harness.handle_envelope(&envelope) else {
        panic!("the shell report did not post");
    };

    assert_ne!(agent_job.job_id, shell_job.job_id, "a new job was posted");
    assert_eq!(
        harness.state.job_ids().unwrap(),
        vec![shell_job.job_id.clone()],
        "the agent's job should have gone with the handover"
    );
    assert_eq!(
        agent_job.group, shell_job.group,
        "both notifications share the pane's group, which is why the old job had to go"
    );
}

/// If the notifier never starts there is nothing on screen, so the job must
/// not linger and pretend there is.
#[test]
fn a_notification_that_could_not_be_posted_leaves_no_job() {
    let mut harness = Harness::answering("post_fails", "w1:p1", "pane-get-unfocused");
    harness.spy = Spy::failing();
    harness.remember_agents(&["claude"]);

    let outcome = harness.handle("agent/blocked");
    assert!(
        matches!(outcome, Outcome::Failed(_)),
        "a failed spawn should be reported: {outcome:?}"
    );
    assert_eq!(
        harness.state.job_ids().unwrap(),
        Vec::new(),
        "no job should be left for a notification that never appeared"
    );

    let Ok(Loaded::Found(record)) = harness.state.pane_record("w1:p1") else {
        panic!("the pane record should still be there");
    };
    assert_eq!(
        record.live_job, None,
        "live_job should not point at a banner that never existed"
    );
}

/// The dedup rule must not be primed by a notification that failed to post.
#[test]
fn a_failed_post_does_not_suppress_the_next_event() {
    let mut harness = Harness::answering("post_fails_twice", "w1:p1", "pane-get-unfocused");
    harness.spy = Spy::failing();
    harness.remember_agents(&["claude"]);
    assert!(matches!(
        harness.handle("agent/blocked"),
        Outcome::Failed(_)
    ));

    harness.spy = Spy::default();
    let second = harness.handle("agent/blocked");
    assert!(
        matches!(second, Outcome::Posted(_)),
        "the retry should post rather than dedupe: {second:?}"
    );
}
