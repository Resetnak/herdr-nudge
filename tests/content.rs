//! Banner text, composed from the captured events.

mod support;

use herdr_nudge::classify::PaneKind;
use herdr_nudge::content::compose;
use herdr_nudge::event::EventData;
use support::Fixture;

/// The status event out of a fixture, so a test reads as "this capture".
fn status(name: &str) -> herdr_nudge::event::StatusEvent {
    match Fixture::load(name).envelope().data {
        EventData::PaneAgentStatusChanged(event) => event,
        other => panic!("{name} is not a status event: {other:?}"),
    }
}

#[test]
fn an_agent_label_is_capitalised_for_the_title() {
    let event = status("agent/blocked");
    let content = compose(PaneKind::Agent, &event, Some("herdr-nudge"), None);
    assert_eq!(content.title, "Claude · blocked", "agent/blocked title");
    assert_eq!(
        content.subtitle, "herdr-nudge",
        "agent/blocked subtitle is the workspace label"
    );
}

#[test]
fn a_shell_command_keeps_the_name_as_typed() {
    let event = status("shell/done-unwatched-after-handover");
    let content = compose(PaneKind::Shell, &event, Some("w3"), None);
    assert_eq!(
        content.title, "make · done",
        "shell/done-unwatched-after-handover title keeps the lowercase command"
    );
}

#[test]
fn the_panes_own_word_beats_ours() {
    let event = status("shell/idle-with-title-labels");
    let content = compose(PaneKind::Shell, &event, None, None);
    assert_eq!(
        content.title, "make · finished",
        "shell/idle-with-title-labels state_labels[idle] is \"finished\""
    );
}

/// The label lives under `idle` even after Herdr rewrote the status to
/// `done`, which is the only way a failure is visible.
#[test]
fn a_failed_command_reads_failed_although_the_status_says_done() {
    let event = status("shell/done-unwatched-failed");
    assert_eq!(
        event.agent_status.as_str(),
        "done",
        "shell/done-unwatched-failed agent_status"
    );
    let content = compose(PaneKind::Shell, &event, None, None);
    assert_eq!(
        content.title, "make · failed",
        "shell/done-unwatched-failed title, from state_labels[idle]"
    );
}

#[test]
fn the_message_is_the_reporters_own_title() {
    let event = status("shell/done-unwatched-failed");
    let content = compose(PaneKind::Shell, &event, None, Some("a terminal title"));
    assert_eq!(
        content.message, "make test · exit 2 · 1m04s",
        "shell/done-unwatched-failed title field beats terminal_title_stripped"
    );
}

#[test]
fn without_a_title_the_message_falls_back_to_the_terminal_title() {
    let event = status("agent/blocked");
    assert!(
        event.title.is_none(),
        "agent/blocked was captured with no title field"
    );
    let content = compose(
        PaneKind::Agent,
        &event,
        None,
        Some("a stripped terminal title"),
    );
    assert_eq!(
        content.message, "a stripped terminal title",
        "agent/blocked message"
    );
}

#[test]
fn with_neither_the_message_is_the_pane_id() {
    let event = status("agent/blocked");
    let content = compose(PaneKind::Agent, &event, None, None);
    assert_eq!(
        content.message, "w1:p1",
        "agent/blocked message falls back to pane id"
    );
}

#[test]
fn the_subtitle_falls_back_to_the_workspace_id() {
    let event = status("agent/done");
    let content = compose(PaneKind::Agent, &event, None, None);
    assert_eq!(
        content.subtitle, "w1",
        "agent/done subtitle with no workspace label"
    );
}

/// `display_agent` is the reporter's own spelling, so it is used verbatim
/// rather than capitalised.
#[test]
fn a_display_agent_is_used_as_sent() {
    let event = status("shell/idle-with-title-labels");
    assert_eq!(
        event.display_agent.as_deref(),
        Some("make"),
        "shell/idle-with-title-labels display_agent"
    );
    let content = compose(PaneKind::Agent, &event, None, None);
    assert_eq!(
        content.title, "make · finished",
        "display_agent is not capitalised even on an agent pane"
    );
}

#[test]
fn every_captured_status_event_composes_without_panicking() {
    for fixture in Fixture::all() {
        let EventData::PaneAgentStatusChanged(event) = fixture.envelope().data else {
            continue;
        };
        for kind in [PaneKind::Agent, PaneKind::Shell] {
            let content = compose(kind, &event, None, None);
            assert!(
                !content.title.is_empty() && !content.message.is_empty(),
                "{}: empty title or message for {kind:?}",
                fixture.name
            );
        }
    }
}
