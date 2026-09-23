//! Which macOS app to bring forward with `open -b` before a click focuses the
//! pane, and whether that app is the one in front.
//!
//! Herdr never tells a plugin which terminal it is running in. But the
//! `herdr server` process carries the `__CFBundleIdentifier` of the terminal
//! it was started from, and hooks are its children, so they have it too
//! (seen on Herdr 0.9.0 under Ghostty). A server started in one terminal and
//! attached from another names the first; `default_terminal` is for that.

use std::path::Path;

use crate::config::Config;
use crate::process::Runner;

const LSAPPINFO: &str = "/usr/bin/lsappinfo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSource {
    DefaultConfig,
    /// `__CFBundleIdentifier` from the hook's environment.
    ServerEnv,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// `None` means the click focuses the pane without bringing any app
    /// forward. The notification is still sent.
    pub bundle_id: Option<String>,
    pub source: TerminalSource,
}

/// `default_terminal`, then the server's own terminal. One answer for every
/// pane: there is one client, so there is one terminal.
pub fn resolve(config: &Config, server_env: Option<&str>) -> Resolution {
    let (bundle_id, source) = if let Some(b) = &config.default_terminal {
        (Some(b.as_str()), TerminalSource::DefaultConfig)
    } else if let Some(b) = server_env {
        (Some(b), TerminalSource::ServerEnv)
    } else {
        (None, TerminalSource::Unresolved)
    };
    Resolution {
        bundle_id: bundle_id.map(str::to_owned),
        source,
    }
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
