//! Herdr Nudge: macOS notifications for Herdr panes.
//!
//! [`cli`] reads the arguments, [`event`] parses what Herdr sends, and
//! [`context`] holds the workspace info and the paths we were given.
//! [`config`] is the user's settings, [`state`] our own files, [`herdr`]
//! every question we ask Herdr, [`classify`] whether a pane is an agent or a
//! shell command, and [`terminal`] which app to bring forward on a click.
//! [`content`] writes the three lines of a banner and [`notifier`] posts it.
//! [`handler`] is what happens on an event, and [`click`] what happens when
//! the notification is clicked. [`shell_hook`] installs the zsh hook that
//! reports long shell commands. [`doctor`] checks the setup.

pub mod classify;
pub mod cli;
pub mod click;
pub mod config;
pub mod content;
pub mod context;
pub mod doctor;
pub mod event;
pub mod handler;
pub mod herdr;
pub mod notifier;
pub mod process;
pub mod shell_hook;
pub mod state;
pub mod terminal;

use context::Context;
use event::{Envelope, EventData};

/// One line per event on stderr, which is where `herdr plugin log` reads
/// from.
///
/// Values go through `{:?}` so a title containing quotes or newlines stays on
/// one line and stays quoted.
pub fn event_summary(envelope: &Envelope, context: Option<&Context>) -> String {
    let mut out = format!("event={}", envelope.event);

    match &envelope.data {
        EventData::PaneAgentStatusChanged(e) => {
            out.push_str(&format!(
                " pane={} workspace={} status={}",
                e.pane_id, e.workspace_id, e.agent_status
            ));
            if let Some(agent) = &e.agent {
                out.push_str(&format!(" agent={agent:?}"));
            }
            if let Some(display) = &e.display_agent {
                out.push_str(&format!(" display_agent={display:?}"));
            }
            if let Some(title) = &e.title {
                out.push_str(&format!(" title={title:?}"));
            }
            for (state, label) in &e.state_labels {
                out.push_str(&format!(" label[{state}]={label:?}"));
            }
        }
        EventData::PaneAgentDetected(e) => {
            out.push_str(&format!(" pane={} workspace={}", e.pane_id, e.workspace_id));
            if let Some(agent) = &e.agent {
                out.push_str(&format!(" agent={agent:?}"));
            }
            if e.released {
                out.push_str(" released=true");
            }
            if let Some(status) = e.final_status {
                out.push_str(&format!(" final_status={status}"));
            }
        }
        EventData::PaneFocused(p) | EventData::PaneClosed(p) => {
            out.push_str(&format!(" pane={} workspace={}", p.pane_id, p.workspace_id));
        }
        EventData::Other => out.push_str(" (not handled)"),
    }

    if let Some(ctx) = context {
        out.push_str(&format!(" ws_label={:?}", ctx.workspace_display()));
        if let Some(tab) = &ctx.tab_id {
            out.push_str(&format!(" tab={tab}"));
        }
    }

    out
}
