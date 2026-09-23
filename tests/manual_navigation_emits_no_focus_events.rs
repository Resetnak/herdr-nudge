//! Moving around Herdr by hand produces no focus events.
//!
//! So nothing can hang off `pane.focused`: the only one we'll ever see is
//! the one our own click causes, never the user coming back to a pane.
//!
//! The capture is someone navigating for three minutes, first with the mouse
//! and then the keyboard, across panes, tabs and workspaces, marking each
//! step as they went.

mod support;

use support::{mark_index, raw_log};

const LOG: &str = "events-2026-09-18-gaps.log";
const FOCUS_EVENTS: [&str; 3] = ["pane.focused", "tab.focused", "workspace.focused"];

#[test]
fn no_focus_event_arrives_while_the_user_navigates_by_hand() {
    let records = raw_log(LOG);
    let start = mark_index(&records, "gap4 phase 1 start");
    let end = mark_index(&records, "gap4 phase 2 end");
    assert!(start < end, "marks are out of order in {LOG}");

    let span = &records[start + 1..end];

    // Without this the test would pass on an empty range, which is what
    // we'd get if the marks were renamed or the log truncated.
    let events: Vec<&support::Record> = span.iter().filter(|r| !r.is_mark()).collect();
    assert!(
        events.len() >= 10,
        "expected the navigation run to still contain events, found {}",
        events.len()
    );

    let focus: Vec<String> = events
        .iter()
        .filter(|r| FOCUS_EVENTS.contains(&r.kind.as_str()))
        .map(|r| format!("{}:{} {}", LOG, r.line, r.kind))
        .collect();

    assert!(
        focus.is_empty(),
        "manual navigation emitted focus events:\n  {}",
        focus.join("\n  ")
    );
}

/// A programmatic focus does emit all three, so the test above would have
/// seen them if manual navigation produced any.
#[test]
fn a_programmatic_focus_emits_all_three() {
    let kinds: Vec<String> = raw_log("events-2026-09-20-socket-focus.log")
        .into_iter()
        .filter(|r| !r.is_mark())
        .map(|r| r.kind)
        .collect();

    for event in FOCUS_EVENTS {
        assert!(
            kinds.iter().any(|k| k == event),
            "no {event} in the socket-focus capture: {kinds:?}"
        );
    }
}
