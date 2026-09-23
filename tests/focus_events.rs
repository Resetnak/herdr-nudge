//! Which focus events Herdr sends when the user moves around, by version.
//!
//! 0.9.0 sent none for manual navigation, only for focus asked for through
//! the CLI or the socket. 0.9.1 sends `pane.focused`, `tab.focused` and
//! `workspace.focused` for every move. The plugin takes a banner down on
//! `pane.focused`, so on 0.9.1 going to a pane clears its banner and on
//! 0.9.0 nothing does. Neither version sends anything when the user comes
//! back to the terminal app with the pane already focused.
//!
//! Both captures are someone clicking or typing in the Herdr TUI, marking
//! each step first with `tools/probe/mark.sh`.

mod support;

use support::{Record, mark_index, raw_log};

const LOG_0_9_0: &str = "events-2026-09-18-gaps.log";
const LOG_0_9_1: &str = "events-2026-09-23-herdr-0.9.1.log";
const FOCUS_EVENTS: [&str; 3] = ["pane.focused", "tab.focused", "workspace.focused"];

/// The records strictly between the marks containing `from` and `to`.
fn between(log: &str, from: &str, to: &str) -> Vec<Record> {
    let records = raw_log(log);
    let start = mark_index(&records, from);
    let end = mark_index(&records, to);
    assert!(
        start < end,
        "marks {from:?} and {to:?} are out of order in {log}"
    );
    records[start + 1..end].to_vec()
}

fn focus_events(records: &[Record]) -> Vec<&Record> {
    records
        .iter()
        .filter(|r| FOCUS_EVENTS.contains(&r.kind.as_str()))
        .collect()
}

#[test]
fn on_0_9_0_manual_navigation_sends_no_focus_event() {
    let span = between(LOG_0_9_0, "gap4 phase 1 start", "gap4 phase 2 end");

    // Without this the test would pass on an empty range, which is what
    // we'd get if the marks were renamed or the log truncated.
    let events = span.iter().filter(|r| !r.is_mark()).count();
    assert!(
        events >= 10,
        "expected the navigation run to still contain events, found {events}"
    );

    let focus: Vec<String> = focus_events(&span)
        .iter()
        .map(|r| format!("{LOG_0_9_0}:{} {}", r.line, r.kind))
        .collect();
    assert!(
        focus.is_empty(),
        "0.9.0 manual navigation sent focus events:\n  {}",
        focus.join("\n  ")
    );
}

/// Eight clicks: to another tab by its tab and back, the same by the agent
/// list and back, to the other pane in the tab and back, to another
/// workspace and back. Each one sends all three.
#[test]
fn on_0_9_1_every_manual_move_sends_all_three() {
    let span = between(LOG_0_9_1, "[manual] navigation round", "[manual] Cmd-Tab");
    let focused: Vec<&str> = span
        .iter()
        .filter(|r| r.kind == "pane.focused")
        .map(|r| r.rest.as_str())
        .collect();
    assert_eq!(
        focused.len(),
        8,
        "pane.focused in the 0.9.1 navigation round: {focused:?}"
    );
    for event in FOCUS_EVENTS {
        let count = span.iter().filter(|r| r.kind == event).count();
        assert_eq!(count, 8, "{event} in the 0.9.1 navigation round");
    }
    for pane in ["w3:p9", "w3:p2", "w1:p2", "w3:p1"] {
        assert!(
            focused
                .iter()
                .any(|json| json.contains(&format!("\"pane_id\":\"{pane}\""))),
            "no pane.focused for {pane} in the 0.9.1 navigation round"
        );
    }
}

/// Cmd-Tab to another app and back, the pane focused throughout. Nothing
/// focus-related arrives, so a banner for the focused pane can't be taken
/// down by coming back to it this way.
#[test]
fn on_0_9_1_coming_back_to_the_terminal_sends_nothing() {
    let span = between(
        LOG_0_9_1,
        "[manual] Cmd-Tab",
        "[programmatic] herdr tab focus w3:t5",
    );
    let events: Vec<&Record> = span.iter().filter(|r| !r.is_mark()).collect();
    assert!(
        focus_events(&span).is_empty(),
        "Cmd-Tab sent focus events: {events:?}"
    );
    // What did arrive is this session's own agent pane changing status.
    assert!(
        events
            .iter()
            .all(|r| r.kind == "pane.agent_status_changed" && r.rest.contains("\"w3:p1\"")),
        "unexpected events around Cmd-Tab: {events:?}"
    );
}

/// `w3:p9` went `done` while the terminal was in the background with that
/// pane focused. The user came back with Cmd-Tab, which turned it `idle`
/// (seen by polling `herdr pane get`, not in this log), and nothing about
/// `w3:p9` was sent from then to the end of the capture. So a banner on a
/// focused pane stays up after the user comes back to it.
#[test]
fn on_0_9_1_the_done_to_idle_flip_on_return_sends_nothing() {
    let records = raw_log(LOG_0_9_1);
    let done = records
        .iter()
        .position(|r| {
            r.kind == "pane.agent_status_changed"
                && r.rest.contains("\"pane_id\":\"w3:p9\"")
                && r.rest.contains("\"agent_status\":\"done\"")
        })
        .expect("w3:p9 going done in the 0.9.1 capture");
    let later: Vec<&Record> = records[done + 1..]
        .iter()
        .filter(|r| r.kind == "pane.agent_status_changed" && r.rest.contains("\"w3:p9\""))
        .collect();
    assert!(
        later.is_empty(),
        "status events for w3:p9 after its done: {later:?}"
    );
}

/// A programmatic focus sends all three on both versions, so the 0.9.0 test
/// above would have seen them if manual navigation produced any.
#[test]
fn a_programmatic_focus_sends_all_three_on_both_versions() {
    let socket_0_9_0: Vec<String> = raw_log("events-2026-09-20-socket-focus.log")
        .into_iter()
        .filter(|r| !r.is_mark())
        .map(|r| r.kind)
        .collect();
    let socket_0_9_1: Vec<String> = between(
        LOG_0_9_1,
        "[programmatic] socket pane.focus w3:p9",
        "[programmatic] socket pane.focus w3:p1, back",
    )
    .into_iter()
    .map(|r| r.kind)
    .collect();

    for (version, kinds) in [("0.9.0", &socket_0_9_0), ("0.9.1", &socket_0_9_1)] {
        for event in FOCUS_EVENTS {
            assert!(
                kinds.iter().any(|k| k == event),
                "no {event} in the {version} socket-focus capture: {kinds:?}"
            );
        }
    }
}

/// Focusing the pane that already has focus sends nothing on 0.9.1.
#[test]
fn on_0_9_1_focusing_the_focused_pane_sends_nothing() {
    let span = between(
        LOG_0_9_1,
        "[programmatic] socket pane.focus w3:p1 (the call a click makes), pane already focused",
        "[programmatic] herdr tab create",
    );
    assert!(
        span.is_empty(),
        "socket pane.focus on the focused pane sent: {span:?}"
    );
}
