//! Is this pane an AI agent, or a shell command reporting itself?
//!
//! Herdr reports both the same way. A shell hook that calls
//! `pane report-agent` produces the same `pane.agent_status_changed` event a
//! real agent does, with a command name where the agent label goes, and the
//! event carries nothing that says which it was. Two signals separate them,
//! and either one is enough to say Agent:
//!
//! 1. the label is one Herdr detects by itself (`server agent-manifests`),
//! 2. the pane has an `agent_session`. Agents bind one through their own
//!    Herdr integration. A shell hook could bind one too
//!    (`report-agent --agent-session-id`); ours doesn't, and neither does
//!    any reporter we've captured.
//!
//! Either, not both, because the two mistakes cost different amounts. A shell
//! command read as an agent costs one missed notification. An agent read as a
//! shell command puts a banner up every time it goes idle.
//!
//! Both signals are in `herdr pane get`, the query that also says whether the
//! user is looking at the pane, so classifying costs no extra subprocess.
//! `agent get` carries the same two fields
//! (`tests/fixtures/cli/agent-get-reported-no-session.json` next to
//! `pane-get-reported-no-session.json` — different panes and different
//! claims, but the same shape), and it fails with `agent_not_found` on a pane
//! that never had an agent, where `pane get` still answers.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::herdr::AGENTS_WITHOUT_MANIFEST;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    Agent,
    Shell,
}

/// What decided it. The plugin log shows it with each banner, so a pane
/// classified the wrong way says why rather than leaving the user guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifySignal {
    /// `known_agents_extra` or `known_agents_remove` named the label, and
    /// Herdr's own list would have said otherwise.
    ConfigOverride,
    /// The label is one of Herdr's agent manifests, or one of
    /// `herdr::AGENTS_WITHOUT_MANIFEST`.
    Catalogue,
    /// The pane has an agent session.
    AgentSession,
    /// Both signals were checked and neither fired: a shell command.
    Neither,
    /// The pane query failed and the label isn't in either list.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub kind: PaneKind,
    pub signal: ClassifySignal,
}

impl Classification {
    pub fn is_agent(&self) -> bool {
        self.kind == PaneKind::Agent
    }
}

/// `manifests` is what Herdr detects by itself, as `agents-cache.json` stores
/// it — **not** the catalogue [`Config::agent_catalogue`] builds. The config
/// lists are applied here instead, for two reasons: passing the raw list
/// can't silently drop `known_agents_extra`, and the signal can say which of
/// the two lists decided. Membership works out the same either way, which
/// `tests/classify.rs` checks label by label.
///
/// `session` is whether `pane get` found an `agent_session` — `None` when the
/// query failed, which is different from finding no session.
pub fn classify(
    config: &Config,
    manifests: &BTreeSet<String>,
    agent_label: Option<&str>,
    session: Option<bool>,
) -> Classification {
    if let Some(label) = agent_label {
        // Checked before everything else, because it is the only way to stop
        // a pane that really does have an agent session from being treated
        // as an agent.
        if names(&config.shell.known_agents_remove, label) {
            return decided(PaneKind::Shell, ClassifySignal::ConfigOverride);
        }
        if manifests.contains(label) || AGENTS_WITHOUT_MANIFEST.contains(&label) {
            return decided(PaneKind::Agent, ClassifySignal::Catalogue);
        }
        if names(&config.shell.known_agents_extra, label) {
            return decided(PaneKind::Agent, ClassifySignal::ConfigOverride);
        }
    }

    match session {
        Some(true) => decided(PaneKind::Agent, ClassifySignal::AgentSession),
        Some(false) => decided(PaneKind::Shell, ClassifySignal::Neither),
        // Once `agents-cache.json` exists, a pane Herdr recognises as an
        // agent never gets this far: the manifests answered without asking
        // anything. What is left is a label Herdr doesn't know, which is what
        // a shell hook reports. An agent with its own integration but no
        // manifest lands here too, unless `AGENTS_WITHOUT_MANIFEST` names it,
        // and so does every agent while nothing has written the cache yet.
        // Each reads as a shell command until the query works again.
        None => decided(PaneKind::Shell, ClassifySignal::Unavailable),
    }
}

fn decided(kind: PaneKind, signal: ClassifySignal) -> Classification {
    Classification { kind, signal }
}

fn names(list: &[String], label: &str) -> bool {
    list.iter().any(|entry| entry == label)
}
