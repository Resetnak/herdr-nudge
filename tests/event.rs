//! Every captured event, against the payload types.

mod support;

use herdr_nudge::event::{AgentStatus, Envelope, EventData};
use herdr_nudge::{context::Context, event_summary};
use support::Fixture;

/// Which variant each event name should parse into. Everything else is
/// `Other`: `pane.created` and `tab.created` nest their subject in an
/// object, `tab.focused` and `workspace.focused` have no pane.
fn expected_variant(event: &str) -> &'static str {
    match event {
        "pane.agent_status_changed" => "PaneAgentStatusChanged",
        "pane.agent_detected" => "PaneAgentDetected",
        "pane.focused" => "PaneFocused",
        "pane.closed" => "PaneClosed",
        "tab.closed" => "TabClosed",
        "workspace.closed" => "WorkspaceClosed",
        _ => "Other",
    }
}

fn variant_of(data: &EventData) -> &'static str {
    match data {
        EventData::PaneAgentStatusChanged(_) => "PaneAgentStatusChanged",
        EventData::PaneAgentDetected(_) => "PaneAgentDetected",
        EventData::PaneFocused(_) => "PaneFocused",
        EventData::PaneClosed(_) => "PaneClosed",
        EventData::TabClosed(_) => "TabClosed",
        EventData::WorkspaceClosed(_) => "WorkspaceClosed",
        EventData::Other => "Other",
    }
}

#[test]
fn every_fixture_parses_into_the_right_variant() {
    let fixtures = Fixture::all();
    assert!(
        fixtures.len() >= 32,
        "expected every captured event, found {}",
        fixtures.len()
    );

    for fixture in &fixtures {
        let envelope = fixture.envelope();

        // Underscores in the payload, dots in HERDR_PLUGIN_EVENT. Neither
        // drives the parse, but they should still agree.
        assert_eq!(
            envelope.event,
            fixture.event.replace('.', "_"),
            "{}: envelope.event",
            fixture.name
        );
        assert_eq!(
            variant_of(&envelope.data),
            expected_variant(&fixture.event),
            "{}: wrong variant for {}",
            fixture.name,
            fixture.event
        );
    }
}

#[test]
fn handled_events_agree_with_the_environment_about_their_pane() {
    for fixture in Fixture::all() {
        let envelope = fixture.envelope();
        let Some(pane_id) = envelope.data.pane_id() else {
            continue;
        };
        assert_eq!(
            Some(pane_id),
            fixture.env.get("HERDR_PANE_ID").map(String::as_str),
            "{}: pane_id",
            fixture.name
        );
        assert_eq!(
            envelope.data.workspace_id(),
            fixture.env.get("HERDR_WORKSPACE_ID").map(String::as_str),
            "{}: workspace_id",
            fixture.name
        );
    }
}

/// Only `pane_id`, `workspace_id` and `agent_status` can be required. After
/// Claude's `/exit` there is no `agent` field at all.
#[test]
fn status_event_parses_without_an_agent_field() {
    let fixture = Fixture::load("agent/status-unknown-no-agent-field");
    assert!(
        !fixture.event_json.contains("\"agent\""),
        "fixture no longer covers the missing-agent case"
    );

    let EventData::PaneAgentStatusChanged(event) = fixture.envelope().data else {
        panic!("expected a status event");
    };
    assert_eq!(event.pane_id, "w3:p4");
    assert_eq!(event.agent_status, AgentStatus::Unknown);
    assert_eq!(event.agent, None);
    assert_eq!(event.title, None);
    assert!(event.state_labels.is_empty());
}

/// `blocked-bare` leaves the optional fields out rather than nulling them.
#[test]
fn status_event_parses_with_every_optional_field_missing() {
    let fixture = Fixture::load("shell/blocked-bare");
    for absent in ["title", "display_agent", "state_labels"] {
        assert!(
            !fixture.event_json.contains(absent),
            "fixture no longer covers a missing {absent}"
        );
    }

    let EventData::PaneAgentStatusChanged(event) = fixture.envelope().data else {
        panic!("expected a status event");
    };
    assert_eq!(event.agent.as_deref(), Some("make"));
    assert_eq!(event.agent_status, AgentStatus::Blocked);
    assert_eq!(event.display_agent, None);
    assert_eq!(event.title, None);
    assert!(event.state_labels.is_empty());
}

/// `pane-created` puts everything inside a nested `pane` object, so there is
/// no `pane_id` at the top level. It should parse as `Other` rather than as a
/// variant that would go looking for one.
#[test]
fn nested_pane_object_parses_as_unhandled() {
    let fixture = Fixture::load("lifecycle/pane-created");
    let raw: serde_json::Value = serde_json::from_str(&fixture.event_json).unwrap();
    assert!(
        raw["data"]["pane_id"].is_null() && raw["data"]["pane"]["pane_id"].is_string(),
        "fixture no longer covers the nested-pane shape"
    );

    let envelope = fixture.envelope();
    assert!(matches!(envelope.data, EventData::Other));
    assert_eq!(envelope.data.pane_id(), None);
    assert_eq!(envelope.data.workspace_id(), None);
}

/// Null and missing should behave the same. Herdr has only ever left these
/// fields out, so the nulls here are injected.
#[test]
fn explicit_null_is_read_as_missing() {
    let with_values = Fixture::load("shell/blocked-with-title-labels");

    for field in ["agent", "display_agent", "title", "state_labels"] {
        let json = with_values.event_json_with_null(field);
        let envelope = Envelope::parse(&json)
            .unwrap_or_else(|e| panic!("null {field} should parse like a missing one: {e}"));
        let EventData::PaneAgentStatusChanged(event) = envelope.data else {
            panic!("expected a status event");
        };

        // The required three are untouched whichever field went null.
        assert_eq!(event.pane_id, "w1:p2");
        assert_eq!(event.workspace_id, "w1");
        assert_eq!(event.agent_status, AgentStatus::Blocked);

        match field {
            "agent" => assert_eq!(event.agent, None),
            "display_agent" => assert_eq!(event.display_agent, None),
            "title" => assert_eq!(event.title, None),
            "state_labels" => assert!(event.state_labels.is_empty()),
            _ => unreachable!(),
        }
    }
}

/// Herdr's own `"unknown"` is captured. A status Herdr might add later
/// isn't, so that one is injected. Neither should fail the parse.
#[test]
fn unknown_statuses_parse_and_stay_unknown() {
    let captured = Fixture::load("shell/status-unknown-on-release");
    let EventData::PaneAgentStatusChanged(event) = captured.envelope().data else {
        panic!("expected a status event");
    };
    assert_eq!(event.agent_status, AgentStatus::Unknown);

    let invented = captured.event_json_with("agent_status", "reviewing".into());
    let envelope = Envelope::parse(&invented).expect("a future status must not fail the parse");
    let EventData::PaneAgentStatusChanged(event) = envelope.data else {
        panic!("expected a status event");
    };
    assert_eq!(event.agent_status, AgentStatus::Unknown);
}

/// Same again for an event type we don't know about.
#[test]
fn unknown_event_types_parse_as_other() {
    let fixture = Fixture::load("agent/blocked");
    let json = fixture.event_json_with("type", "pane_teleported".into());
    let envelope = Envelope::parse(&json).expect("a future event must not fail the parse");
    assert!(matches!(envelope.data, EventData::Other));
}

/// Herdr rewrites an unwatched completion from `idle` to `done` but keeps
/// the state labels, so a failed command is recognised by
/// `state_labels["idle"]` rather than by its status.
#[test]
fn state_labels_survive_the_idle_to_done_rewrite() {
    let cases = [
        ("shell/done-unwatched-failed", AgentStatus::Done, "failed"),
        (
            "shell/done-unwatched-after-handover",
            AgentStatus::Done,
            "done",
        ),
        (
            "shell/idle-with-title-labels",
            AgentStatus::Idle,
            "finished",
        ),
    ];

    for (name, status, label) in cases {
        let EventData::PaneAgentStatusChanged(event) = Fixture::load(name).envelope().data else {
            panic!("{name}: expected a status event");
        };
        assert_eq!(event.agent_status, status, "{name}");
        assert_eq!(
            event.state_labels.get("idle").map(String::as_str),
            Some(label),
            "{name}: state_labels[idle]"
        );
        assert!(event.title.is_some(), "{name}: title");
    }
}

/// A completion the user watched arrives as `idle`, an unwatched one as
/// `done`. Herdr already knows where the user was, so this is a second check
/// on top of asking it.
#[test]
fn watched_and_unwatched_completions_differ() {
    let watched = Fixture::load("agent/idle-watched-completion");
    let unwatched = Fixture::load("agent/done");
    assert_eq!(status_of(&watched), AgentStatus::Idle);
    assert_eq!(status_of(&unwatched), AgentStatus::Done);
    // Only a real person moving away produces this.
    assert_eq!(watched.provenance, "manual");
    assert_eq!(unwatched.provenance, "manual");
}

#[test]
fn agent_detected_carries_claims_and_releases() {
    let EventData::PaneAgentDetected(claim) = Fixture::load("detected/shell-claim").envelope().data
    else {
        panic!("expected a detected event");
    };
    assert_eq!(claim.agent.as_deref(), Some("make"));
    assert!(!claim.released, "a claim has no released field at all");
    assert_eq!(claim.final_status, None);

    let EventData::PaneAgentDetected(release) = Fixture::load("detected/agent-release-on-exit")
        .envelope()
        .data
    else {
        panic!("expected a detected event");
    };
    assert!(release.released);
    assert_eq!(release.final_status, Some(AgentStatus::Idle));

    let EventData::PaneAgentDetected(shell_release) =
        Fixture::load("detected/shell-release").envelope().data
    else {
        panic!("expected a detected event");
    };
    assert!(shell_release.released);
    assert_eq!(shell_release.final_status, Some(AgentStatus::Unknown));
}

#[test]
fn focus_and_close_events_carry_only_a_pane_reference() {
    let EventData::PaneFocused(focused) = Fixture::load("focus/socket-pane-focus-pane-focused")
        .envelope()
        .data
    else {
        panic!("expected pane.focused");
    };
    assert_eq!(focused.pane_id, "w3:p2");
    assert_eq!(focused.workspace_id, "w3");

    let EventData::PaneClosed(closed) = Fixture::load("lifecycle/pane-closed").envelope().data
    else {
        panic!("expected pane.closed");
    };
    assert_eq!(closed.pane_id, "w1:p1");
    assert_eq!(closed.workspace_id, "w1");
}

/// Neither close event names a pane, on either version, so the handler has
/// to ask Herdr which ones went.
#[test]
fn tab_and_workspace_close_events_carry_ids_but_no_panes() {
    for (fixture, tab_id, workspace_id) in [
        ("lifecycle/tab-closed", "w3:tJ", "w3"),
        ("lifecycle/tab-closed-by-cli", "w3:tH", "w3"),
        ("lifecycle/tab-closed-by-cli-0.9.0", "w1:t2", "w1"),
        ("lifecycle/tab-closed-by-pane-move", "w7:t2", "w7"),
    ] {
        let envelope = Fixture::load(fixture).envelope();
        let EventData::TabClosed(closed) = &envelope.data else {
            panic!("{fixture}: expected tab.closed");
        };
        assert_eq!(closed.tab_id, tab_id, "{fixture}: tab_id");
        assert_eq!(closed.workspace_id, workspace_id, "{fixture}: workspace_id");
        assert_eq!(envelope.data.pane_id(), None, "{fixture}: pane_id");
    }
    for (fixture, workspace_id) in [
        ("lifecycle/workspace-closed", "w6"),
        ("lifecycle/workspace-closed-by-cli", "w4"),
        ("lifecycle/workspace-closed-by-cli-0.9.0", "w2"),
    ] {
        let envelope = Fixture::load(fixture).envelope();
        let EventData::WorkspaceClosed(closed) = &envelope.data else {
            panic!("{fixture}: expected workspace.closed");
        };
        assert_eq!(closed.workspace_id, workspace_id, "{fixture}: workspace_id");
        assert_eq!(envelope.data.pane_id(), None, "{fixture}: pane_id");
    }
}

/// One line per event, whatever the payload. A title is arbitrary text and
/// could contain anything, including newlines.
#[test]
fn every_fixture_logs_exactly_one_line() {
    for fixture in Fixture::all() {
        let envelope = fixture.envelope();
        let context = Context::parse(fixture.context_json()).ok();
        let summary = event_summary(&envelope, context.as_ref());

        assert!(
            !summary.contains('\n'),
            "{}: summary spans lines: {summary}",
            fixture.name
        );
        assert!(
            summary.starts_with(&format!("event={}", envelope.event)),
            "{}: summary does not name its event: {summary}",
            fixture.name
        );
        if let Some(pane_id) = envelope.data.pane_id() {
            assert!(
                summary.contains(&format!("pane={pane_id}")),
                "{}: summary omits the pane: {summary}",
                fixture.name
            );
        }
    }
}

#[test]
fn a_summary_shows_the_fields_a_notification_will_be_built_from() {
    let fixture = Fixture::load("shell/done-unwatched-failed");
    let summary = event_summary(&fixture.envelope(), Some(&fixture.context()));
    assert_eq!(
        summary,
        "event=pane_agent_status_changed pane=w3:p3 workspace=w3 status=done \
         agent=\"make\" display_agent=\"make\" title=\"make test · exit 2 · 1m04s\" \
         label[idle]=\"failed\" ws_label=\"herdr-nudge\" tab=w3:t2"
    );
}

fn status_of(fixture: &Fixture) -> AgentStatus {
    match fixture.envelope().data {
        EventData::PaneAgentStatusChanged(event) => event.agent_status,
        _ => panic!("{}: expected a status event", fixture.name),
    }
}
