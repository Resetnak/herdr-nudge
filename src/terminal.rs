//! Which macOS app hosts a workspace, so a click can bring it forward with
//! `open -b` before focusing the pane.
//!
//! Herdr never tells a plugin this, so it comes from the config or from
//! watching which terminal is in front while the user is on a pane. Config
//! always wins. Learning only fills gaps, because Herdr emits no focus event
//! on manual navigation (`tests/manual_navigation_emits_no_focus_events.rs`),
//! so there's no reliable moment to learn at.

use std::path::Path;

use crate::config::Config;
use crate::process::Runner;
use crate::state::{FocusOrigin, TerminalMemory};

const LSAPPINFO: &str = "/usr/bin/lsappinfo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSource {
    WorkspaceOverride,
    DefaultConfig,
    Learned,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// `None` means the click focuses the pane without bringing any app
    /// forward. The notification is still sent.
    pub bundle_id: Option<String>,
    pub source: TerminalSource,
}

/// First match wins: `[workspaces]`, then `default_terminal`, then what we
/// learned.
pub fn resolve(config: &Config, memory: &TerminalMemory, workspace_id: &str) -> Resolution {
    let (bundle_id, source) = if let Some(b) = config.workspaces.get(workspace_id) {
        (Some(b.as_str()), TerminalSource::WorkspaceOverride)
    } else if let Some(b) = &config.default_terminal {
        (Some(b.as_str()), TerminalSource::DefaultConfig)
    } else if let Some(b) = memory.get(workspace_id) {
        (Some(b), TerminalSource::Learned)
    } else {
        (None, TerminalSource::Unresolved)
    };
    Resolution {
        bundle_id: bundle_id.map(str::to_owned),
        source,
    }
}

/// Why nothing was learned. Handy in logs and in `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The config already names a terminal for this workspace.
    Configured,
    /// We focused a pane in this workspace from a click in the last 15 s,
    /// so what's in front may be Notification Center, not the terminal.
    RecentClick,
    PaneNotFocused,
    /// `herdr pane get` failed.
    FocusUnknown,
    FrontmostUnknown,
    /// What's in front isn't in `terminal_allowlist`.
    NotATerminal(String),
    AlreadyKnown,
}

/// Decides whether to learn `workspace_id`'s terminal, and returns the
/// bundle id to store if so.
///
/// The two lookups are closures because they cost a subprocess each, and
/// most calls stop before needing them: whenever the config has a value,
/// and the frontmost app isn't asked about unless the pane is focused.
pub fn learn(
    config: &Config,
    memory: &TerminalMemory,
    origin: &FocusOrigin,
    workspace_id: &str,
    now_ms: u64,
    pane_focused: impl FnOnce() -> Option<bool>,
    frontmost: impl FnOnce() -> Option<String>,
) -> Result<String, Skip> {
    if config.workspaces.contains_key(workspace_id) || config.default_terminal.is_some() {
        return Err(Skip::Configured);
    }
    if origin.is_recent(workspace_id, now_ms) {
        return Err(Skip::RecentClick);
    }
    match pane_focused() {
        Some(true) => {}
        Some(false) => return Err(Skip::PaneNotFocused),
        None => return Err(Skip::FocusUnknown),
    }
    let bundle_id = frontmost().ok_or(Skip::FrontmostUnknown)?;
    if !config.terminal_allowlist.contains(&bundle_id) {
        return Err(Skip::NotATerminal(bundle_id));
    }
    if memory.get(workspace_id) == Some(bundle_id.as_str()) {
        return Err(Skip::AlreadyKnown);
    }
    Ok(bundle_id)
}

/// The frontmost app's bundle id, from `lsappinfo`. It needs no permission,
/// where asking System Events through `osascript` puts up an Automation
/// prompt.
pub fn frontmost_bundle_id(runner: &impl Runner) -> Option<String> {
    let lsappinfo = Path::new(LSAPPINFO);
    let front = runner.run(lsappinfo, &["front"]).ok()?;
    let asn = front.stdout.trim();
    if !front.success() || !asn.starts_with("ASN:") {
        return None;
    }
    let info = runner
        .run(lsappinfo, &["info", "-only", "bundleid", asn])
        .ok()?;
    if !info.success() {
        return None;
    }
    parse_bundle_id(&info.stdout)
}

/// `"CFBundleIdentifier"="com.mitchellh.ghostty"` gives the id. An app
/// that has quit gives `"CFBundleIdentifier"=[ NULL ]`, and an ASN that
/// never existed gives nothing at all.
pub fn parse_bundle_id(stdout: &str) -> Option<String> {
    let value = stdout.trim().strip_prefix("\"CFBundleIdentifier\"=")?;
    let id = value.strip_prefix('"')?.strip_suffix('"')?;
    (!id.is_empty() && !id.contains('"')).then(|| id.to_owned())
}
