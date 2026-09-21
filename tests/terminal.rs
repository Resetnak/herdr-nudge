//! Which terminal a click brings forward: resolution order, when learning is
//! allowed, and reading the frontmost app from `lsappinfo`.

mod support;

use std::cell::Cell;

use herdr_nudge::config::Config;
use herdr_nudge::herdr::Cli;
use herdr_nudge::state::{FOCUS_ORIGIN_TTL_MS, FocusOrigin, TerminalMemory};
use herdr_nudge::terminal::{
    Resolution, Skip, TerminalSource, frontmost_bundle_id, learn, parse_bundle_id, resolve,
};
use support::{Fixture, Recorded, Replay};

const GHOSTTY: &str = "com.mitchellh.ghostty";
const ITERM: &str = "com.googlecode.iterm2";
const TERMINAL: &str = "com.apple.Terminal";

fn memory_with(workspace: &str, bundle_id: &str) -> TerminalMemory {
    let mut memory = TerminalMemory::default();
    memory.set(workspace, bundle_id, 1);
    memory
}

fn resolved(bundle_id: &str, source: TerminalSource) -> Resolution {
    Resolution {
        bundle_id: Some(bundle_id.to_owned()),
        source,
    }
}

/// Each source is shadowed by every one above it.
#[test]
fn resolution_order_is_override_then_default_then_learned() {
    let memory = memory_with("w8", TERMINAL);
    let with_default = Config::parse(&format!(
        "default_terminal = \"{GHOSTTY}\"\n[workspaces]\nw8 = \"{ITERM}\"\n"
    ))
    .unwrap();
    let without_default = Config::parse(&format!("[workspaces]\nw1 = \"{ITERM}\"\n")).unwrap();

    assert_eq!(
        resolve(&with_default, &memory, "w8"),
        resolved(ITERM, TerminalSource::WorkspaceOverride),
        "override beats default and learned"
    );
    assert_eq!(
        resolve(&with_default, &memory, "w9"),
        resolved(GHOSTTY, TerminalSource::DefaultConfig),
        "default beats nothing"
    );
    assert_eq!(
        resolve(&with_default, &memory_with("w9", TERMINAL), "w9"),
        resolved(GHOSTTY, TerminalSource::DefaultConfig),
        "default beats learned"
    );
    assert_eq!(
        resolve(&without_default, &memory, "w8"),
        resolved(TERMINAL, TerminalSource::Learned),
        "learned when the config says nothing for w8"
    );
    assert_eq!(
        resolve(&without_default, &memory, "w9"),
        Resolution {
            bundle_id: None,
            source: TerminalSource::Unresolved
        }
    );
}

/// The workspace ids in real events are what the config is keyed by.
#[test]
fn a_captured_events_workspace_resolves_by_its_override() {
    let fixture = Fixture::load("agent/blocked");
    let workspace = fixture.envelope().data.workspace_id().unwrap().to_owned();
    let config = Config::parse(&format!("[workspaces]\n{workspace} = \"{ITERM}\"\n")).unwrap();

    assert_eq!(
        resolve(&config, &TerminalMemory::default(), &workspace),
        resolved(ITERM, TerminalSource::WorkspaceOverride),
        "agent/blocked: workspace {workspace}"
    );
}

/// Lookups that record whether they ran, so a test can check that learning
/// stops before the subprocesses it doesn't need.
struct Probe {
    focused: Option<bool>,
    frontmost: Option<&'static str>,
    asked_focus: Cell<bool>,
    asked_frontmost: Cell<bool>,
}

impl Probe {
    fn new(focused: Option<bool>, frontmost: Option<&'static str>) -> Probe {
        Probe {
            focused,
            frontmost,
            asked_focus: Cell::new(false),
            asked_frontmost: Cell::new(false),
        }
    }

    fn learn(
        &self,
        config: &Config,
        memory: &TerminalMemory,
        origin: &FocusOrigin,
        now_ms: u64,
    ) -> Result<String, Skip> {
        learn(
            config,
            memory,
            origin,
            "w1",
            now_ms,
            || {
                self.asked_focus.set(true);
                self.focused
            },
            || {
                self.asked_frontmost.set(true);
                self.frontmost.map(str::to_owned)
            },
        )
    }
}

const NOW: u64 = 1_000_000;

#[test]
fn learns_when_every_condition_holds() {
    let probe = Probe::new(Some(true), Some(GHOSTTY));
    let result = probe.learn(
        &Config::default(),
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(result, Ok(GHOSTTY.to_owned()));
}

/// Config always wins, and there's no point asking Herdr or macOS anything.
#[test]
fn never_learns_over_config() {
    for config in [
        format!("default_terminal = \"{ITERM}\"\n"),
        format!("[workspaces]\nw1 = \"{ITERM}\"\n"),
    ] {
        let probe = Probe::new(Some(true), Some(GHOSTTY));
        let result = probe.learn(
            &Config::parse(&config).unwrap(),
            &TerminalMemory::default(),
            &FocusOrigin::default(),
            NOW,
        );
        assert_eq!(result, Err(Skip::Configured), "{config}");
        assert!(
            !probe.asked_focus.get() && !probe.asked_frontmost.get(),
            "{config}"
        );
    }
}

#[test]
fn an_override_for_another_workspace_does_not_block_learning() {
    let config = Config::parse(&format!("[workspaces]\nw2 = \"{ITERM}\"\n")).unwrap();
    let probe = Probe::new(Some(true), Some(GHOSTTY));
    let result = probe.learn(
        &config,
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(result, Ok(GHOSTTY.to_owned()));
}

#[test]
fn no_learning_right_after_our_own_click() {
    let mut origin = FocusOrigin::default();
    origin.mark("w1", NOW);

    let probe = Probe::new(Some(true), Some(GHOSTTY));
    let during = probe.learn(
        &Config::default(),
        &TerminalMemory::default(),
        &origin,
        NOW + 1,
    );
    assert_eq!(during, Err(Skip::RecentClick));
    assert!(!probe.asked_focus.get());

    let after = Probe::new(Some(true), Some(GHOSTTY)).learn(
        &Config::default(),
        &TerminalMemory::default(),
        &origin,
        NOW + FOCUS_ORIGIN_TTL_MS,
    );
    assert_eq!(after, Ok(GHOSTTY.to_owned()));
}

#[test]
fn no_learning_unless_the_pane_is_focused() {
    for (focused, skip) in [
        (Some(false), Skip::PaneNotFocused),
        (None, Skip::FocusUnknown),
    ] {
        let probe = Probe::new(focused, Some(GHOSTTY));
        let result = probe.learn(
            &Config::default(),
            &TerminalMemory::default(),
            &FocusOrigin::default(),
            NOW,
        );
        assert_eq!(result, Err(skip));
        assert!(
            !probe.asked_frontmost.get(),
            "{focused:?}: asked lsappinfo anyway"
        );
    }
}

#[test]
fn no_learning_from_an_app_that_is_not_a_terminal() {
    let probe = Probe::new(Some(true), Some("com.apple.Safari"));
    let result = probe.learn(
        &Config::default(),
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(
        result,
        Err(Skip::NotATerminal("com.apple.Safari".to_owned()))
    );

    let probe = Probe::new(Some(true), None);
    let result = probe.learn(
        &Config::default(),
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(result, Err(Skip::FrontmostUnknown));
}

#[test]
fn the_allowlist_comes_from_config() {
    let config = Config::parse("terminal_allowlist = [\"com.example.term\"]\n").unwrap();
    let ghostty = Probe::new(Some(true), Some(GHOSTTY)).learn(
        &config,
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(ghostty, Err(Skip::NotATerminal(GHOSTTY.to_owned())));

    let custom = Probe::new(Some(true), Some("com.example.term")).learn(
        &config,
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(custom, Ok("com.example.term".to_owned()));
}

#[test]
fn no_rewrite_when_the_value_is_unchanged() {
    let result = Probe::new(Some(true), Some(GHOSTTY)).learn(
        &Config::default(),
        &memory_with("w1", GHOSTTY),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(result, Err(Skip::AlreadyKnown));

    let changed = Probe::new(Some(true), Some(ITERM)).learn(
        &Config::default(),
        &memory_with("w1", GHOSTTY),
        &FocusOrigin::default(),
        NOW,
    );
    assert_eq!(changed, Ok(ITERM.to_owned()));
}

/// The real lookups, wired to recordings: `pane get` for the focus check
/// and `lsappinfo` for the frontmost app.
#[test]
fn learns_from_recorded_pane_get_and_lsappinfo() {
    let herdr = Replay::new([Recorded::cli("pane-get-focused")]);
    let lsappinfo = Replay::new([
        Recorded::sys("lsappinfo-front"),
        Recorded::sys("lsappinfo-bundleid-ghostty"),
    ]);
    let cli = Cli {
        bin: "/Users/dev/.local/bin/herdr".as_ref(),
        runner: &herdr,
    };

    let result = learn(
        &Config::default(),
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        "w3",
        NOW,
        || cli.pane_get("w3:p1").ok().map(|p| p.focused),
        || frontmost_bundle_id(&lsappinfo),
    );
    assert_eq!(result, Ok(GHOSTTY.to_owned()));
    assert_eq!(
        *lsappinfo.calls.borrow(),
        [
            vec!["/usr/bin/lsappinfo", "front"],
            vec![
                "/usr/bin/lsappinfo",
                "info",
                "-only",
                "bundleid",
                "ASN:0x0-0x64b64b:"
            ]
        ]
    );
}

#[test]
fn an_unfocused_recorded_pane_stops_before_lsappinfo() {
    let herdr = Replay::new([Recorded::cli("pane-get-unfocused")]);
    let lsappinfo = Replay::new([]);
    let cli = Cli {
        bin: "/Users/dev/.local/bin/herdr".as_ref(),
        runner: &herdr,
    };

    let result = learn(
        &Config::default(),
        &TerminalMemory::default(),
        &FocusOrigin::default(),
        "w3",
        NOW,
        || cli.pane_get("w3:p2").ok().map(|p| p.focused),
        || frontmost_bundle_id(&lsappinfo),
    );
    assert_eq!(result, Err(Skip::PaneNotFocused));
    assert_eq!(lsappinfo.call_count(), 0);
}

#[test]
fn bundle_id_parsing() {
    assert_eq!(
        parse_bundle_id(&Recorded::sys("lsappinfo-bundleid-ghostty").stdout).as_deref(),
        Some(GHOSTTY)
    );
    assert_eq!(
        parse_bundle_id(&Recorded::sys("lsappinfo-bundleid-gone").stdout),
        None,
        "lsappinfo-bundleid-gone: [ NULL ]"
    );
    assert_eq!(parse_bundle_id(""), None);
}

#[test]
fn a_front_reply_that_is_not_an_asn_asks_nothing_more() {
    let mut front = Recorded::sys("lsappinfo-front");
    front.stdout = String::new();
    let replay = Replay::new([front]);
    assert_eq!(frontmost_bundle_id(&replay), None);
    assert_eq!(replay.call_count(), 1);
}
