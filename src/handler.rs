//! What we do about one event.
//!
//! The order matters and most of it is about not asking Herdr questions we
//! don't need answered. An event hook runs on every status change, including
//! the `working` churn of an agent that is just thinking, so the first thing
//! checked is whether any trigger set names this status at all. Past that
//! point one `herdr pane get` answers three questions at once: is the user
//! looking at this pane, is it an agent or a shell command, and is this a
//! moment we could learn the workspace's terminal from.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::classify::{self, Classification, ClassifySignal, PaneKind};
use crate::cli::JobId;
use crate::config::Config;
use crate::content;
use crate::context::Context;
use crate::event::{AgentStatus, Envelope, EventData, StatusEvent};
use crate::herdr::{Cli, PaneInfo};
use crate::notifier::{self, Notifier, Post};
use crate::process::{Runner, Spawner};
use crate::state::{self, Job, Loaded, PaneRecord, StateDir, VERSION};
use crate::terminal::{self, Resolution};

pub struct Deps<'a, R: Runner, S: Spawner> {
    pub config: &'a Config,
    pub state: &'a StateDir,
    pub herdr_bin: &'a Path,
    pub socket_path: &'a Path,
    pub notifier_bin: &'a Path,
    /// Our own binary, for the click command. Must be absolute.
    pub self_bin: &'a Path,
    pub plugin_root: &'a Path,
    pub runner: &'a R,
    pub spawner: &'a S,
    pub now_ms: u64,
    pub pid: u32,
}

/// Why an event did or didn't become a notification. `main` logs this and the
/// tests assert on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not `pane.agent_status_changed`. Clearing state for closed and
    /// refocused panes isn't wired up yet.
    NotHandled,
    /// No trigger set names this status, so nothing was asked of Herdr.
    StatusNotWatched(AgentStatus),
    Disabled(PaneKind),
    NotATrigger {
        kind: PaneKind,
        status: AgentStatus,
    },
    IgnoredAgent(String),
    /// `notify_on_failure_only` is on and the command succeeded.
    NotAFailure,
    /// The user is looking at the pane in the frontmost terminal.
    Watching,
    /// A notification for this pane, agent and status is already showing.
    AlreadyShowing(String),
    NotifierMissing(PathBuf),
    /// Our own path can't be put in the click command, so no notification
    /// would be clickable.
    CannotBuildClick(String),
    Failed(String),
    Posted(Posted),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    pub job_id: JobId,
    pub group: String,
    pub kind: PaneKind,
    pub signal: ClassifySignal,
    pub title: String,
}

/// The outcome plus anything worth putting in `herdr plugin log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    pub notes: Vec<String>,
}

impl Report {
    fn new(outcome: Outcome, notes: Vec<String>) -> Report {
        Report { outcome, notes }
    }
}

/// Drops jobs whose clickable time has run out and withdraws their
/// notifications.
///
/// Runs on every event. It is a directory read, not a subprocess, and it only
/// starts the notifier for a job that has actually expired.
pub fn sweep_expired<S: Spawner>(
    state: &StateDir,
    spawner: &S,
    now_ms: u64,
) -> (Vec<JobId>, Vec<String>) {
    let mut swept = Vec::new();
    let mut notes = Vec::new();

    let ids = match state.job_ids() {
        Ok(ids) => ids,
        Err(e) => {
            notes.push(format!("could not list jobs: {e}"));
            return (swept, notes);
        }
    };

    for id in ids {
        let Ok(Loaded::Found(job)) = state.job(&id) else {
            continue;
        };
        if !job.is_expired(now_ms) {
            continue;
        }
        let notifier = Notifier {
            binary: &job.notifier_path,
            spawner,
        };
        if let Err(e) = notifier.remove(&job.group) {
            notes.push(format!("could not remove group {}: {e}", job.group));
        }
        if let Err(e) = state.delete_job(&id) {
            notes.push(format!("could not delete job {id}: {e}"));
        }
        swept.push(id);
    }
    (swept, notes)
}

pub fn handle<R: Runner, S: Spawner>(
    deps: &Deps<R, S>,
    envelope: &Envelope,
    context: Option<&Context>,
) -> Report {
    let mut notes = Vec::new();
    let EventData::PaneAgentStatusChanged(event) = &envelope.data else {
        return Report::new(Outcome::NotHandled, notes);
    };

    // Before anything else, and without asking Herdr: could this status ever
    // produce a notification? The trigger set depends on whether the pane is
    // an agent, and that answer costs a subprocess, so the union of both sets
    // is what gets checked here.
    if !watched_by_any(deps.config, event.agent_status) {
        return Report::new(Outcome::StatusNotWatched(event.agent_status), notes);
    }

    let cli = Cli {
        bin: deps.herdr_bin,
        runner: deps.runner,
    };
    let info = match cli.pane_get(&event.pane_id) {
        Ok(info) => Some(info),
        Err(e) => {
            notes.push(format!("pane get {}: {e}", event.pane_id));
            None
        }
    };

    let remembered = match deps.state.pane_record(&event.pane_id) {
        Ok(Loaded::Found(record)) => Some(record),
        Ok(Loaded::Recovered(r)) => {
            notes.push(format!("pane record unreadable ({}), ignored", r.reason));
            None
        }
        Ok(Loaded::Missing) => None,
        Err(e) => {
            notes.push(format!("pane record: {e}"));
            None
        }
    };

    // The event's own label is the one this status is about. A pane that has
    // just been released has none, and then the query's is the best we have.
    let agent_label = event
        .agent
        .as_deref()
        .or(info.as_ref().and_then(|i| i.agent.as_deref()));

    let classification = classify::classify(
        deps.config,
        &manifests(deps.state, &mut notes),
        agent_label,
        info.as_ref().map(PaneInfo::has_agent_session),
        remembered.as_ref(),
    );

    let mut record = PaneRecord::new(&event.pane_id, agent_label, classification, deps.now_ms);
    // Carry the live notification across, but only if it belongs to this same
    // agent identity.
    record.live_job = remembered
        .as_ref()
        .filter(|r| r.is_about(agent_label))
        .and_then(|r| r.live_job.clone());

    let decision = decide(deps.config, classification, event, agent_label);
    if let Some(outcome) = decision {
        save_record(deps.state, &record, &mut notes);
        return Report::new(outcome, notes);
    }

    // Learning and the visibility check both want to know what app is in
    // front, and both only care when the user is on this pane. So the two
    // `lsappinfo` calls happen at most once, and only then.
    let focused = info.as_ref().map(|i| i.focused);
    let frontmost = if focused == Some(true) {
        terminal::frontmost_bundle_id(deps.runner)
    } else {
        None
    };

    let resolution = resolve_terminal(deps, event, focused, frontmost.as_deref(), &mut notes);

    if focused == Some(true)
        && let (Some(front), Some(bundle)) = (&frontmost, &resolution.bundle_id)
        && front == bundle
    {
        save_record(deps.state, &record, &mut notes);
        return Report::new(Outcome::Watching, notes);
    }

    // A status that repeats is normal: reporting metadata alone emits a
    // status event carrying the unchanged status.
    if let Some(showing) = live_job(deps, &record, event, agent_label) {
        save_record(deps.state, &record, &mut notes);
        return Report::new(Outcome::AlreadyShowing(showing), notes);
    }

    if !deps.notifier_bin.is_file() {
        save_record(deps.state, &record, &mut notes);
        return Report::new(
            Outcome::NotifierMissing(deps.notifier_bin.to_owned()),
            notes,
        );
    }

    let job_id = state::new_job_id(deps.now_ms, deps.pid);
    let execute = match notifier::click_command(deps.self_bin, &job_id) {
        Ok(execute) => execute,
        Err(e) => {
            save_record(deps.state, &record, &mut notes);
            return Report::new(Outcome::CannotBuildClick(e.to_string()), notes);
        }
    };

    let group = notifier::group_for(&event.pane_id);
    let content = content::compose(
        classification.kind,
        event,
        context.map(Context::workspace_display),
        info.as_ref()
            .and_then(|i| i.terminal_title_stripped.as_deref()),
    );

    let job = Job {
        version: VERSION,
        id: job_id.to_string(),
        pane_id: event.pane_id.clone(),
        workspace_id: event.workspace_id.clone(),
        agent_label: agent_label.map(str::to_owned),
        kind: classification.kind,
        status: event.agent_status,
        group: group.clone(),
        bundle_id: resolution.bundle_id.clone(),
        socket_path: deps.socket_path.to_owned(),
        notifier_path: deps.notifier_bin.to_owned(),
        created_at_ms: deps.now_ms,
        expires_at_ms: deps.now_ms.saturating_add(
            deps.config
                .notifications
                .clickable_secs
                .saturating_mul(1000),
        ),
        repeat_after_ms: deps.now_ms.saturating_add(state::REPEAT_AFTER_MS),
    };

    // Written before anything is on screen, because a banner can be clicked
    // the moment it appears and a click with no job file does nothing. The
    // old job is dropped only after this one is safely on disk, so a failed
    // write leaves the notification that is already up still clickable.
    if let Err(e) = deps.state.save_job(&job) {
        save_record(deps.state, &record, &mut notes);
        return Report::new(Outcome::Failed(format!("could not write job: {e}")), notes);
    }
    drop_other_jobs(deps, &event.pane_id, &job_id, &mut notes);
    record.live_job = Some(job.id.clone());
    save_record(deps.state, &record, &mut notes);

    let image = logo_for(deps, classification.kind, agent_label);
    let notifier = Notifier {
        binary: deps.notifier_bin,
        spawner: deps.spawner,
    };
    let post = Post {
        title: &content.title,
        subtitle: &content.subtitle,
        message: &content.message,
        group: &group,
        content_image: image.as_deref(),
        sound: deps.config.notifications.sound,
        execute: &execute,
    };
    if let Err(e) = notifier.post(&post) {
        // Nothing reached the screen, so the job must not stay: it would
        // suppress the next event as a duplicate of a banner that never
        // existed, and later withdraw whatever notification does make it up.
        if let Err(e) = deps.state.delete_job(&job_id) {
            notes.push(format!("could not delete unposted job {job_id}: {e}"));
        }
        record.live_job = None;
        save_record(deps.state, &record, &mut notes);
        return Report::new(
            Outcome::Failed(format!("could not start the notifier: {e}")),
            notes,
        );
    }

    Report::new(
        Outcome::Posted(Posted {
            job_id,
            group,
            kind: classification.kind,
            signal: classification.signal,
            title: content.title,
        }),
        notes,
    )
}

/// Either side of the config could want this status. Both are checked with
/// their `enabled` flag, so a disabled half doesn't buy a subprocess.
fn watched_by_any(config: &Config, status: AgentStatus) -> bool {
    (config.agents.enabled && config.agents.statuses.contains(&status))
        || (config.shell.enabled && config.shell.statuses.contains(&status))
}

/// The checks that only need the config and the event. `None` means carry on.
fn decide(
    config: &Config,
    classification: Classification,
    event: &StatusEvent,
    agent_label: Option<&str>,
) -> Option<Outcome> {
    let (enabled, statuses) = match classification.kind {
        PaneKind::Agent => (config.agents.enabled, &config.agents.statuses),
        PaneKind::Shell => (config.shell.enabled, &config.shell.statuses),
    };
    if !enabled {
        return Some(Outcome::Disabled(classification.kind));
    }
    if !statuses.contains(&event.agent_status) {
        return Some(Outcome::NotATrigger {
            kind: classification.kind,
            status: event.agent_status,
        });
    }

    // `ignore_agents` sits under `[shell]` and is keyed on the label a
    // reporter sends, so it only applies to shell panes.
    if classification.kind == PaneKind::Shell
        && let Some(label) = agent_label
        && config.shell.ignore_agents.iter().any(|a| a == label)
    {
        return Some(Outcome::IgnoredAgent(label.to_owned()));
    }

    // The label survives Herdr rewriting `idle` to `done`, so `idle` is the
    // key to look under for both statuses.
    if classification.kind == PaneKind::Shell
        && config.shell.notify_on_failure_only
        && event.state_labels.get("idle").map(String::as_str) != Some("failed")
    {
        return Some(Outcome::NotAFailure);
    }

    None
}

fn manifests(state: &StateDir, notes: &mut Vec<String>) -> BTreeSet<String> {
    match state.agents_cache() {
        Ok(loaded) => loaded.into_value().agents,
        Err(e) => {
            notes.push(format!("agents cache: {e}"));
            BTreeSet::new()
        }
    }
}

/// Learns the workspace's terminal if this is a moment we can trust, then
/// resolves it. Config always wins, so learning can only fill a gap.
fn resolve_terminal<R: Runner, S: Spawner>(
    deps: &Deps<R, S>,
    event: &StatusEvent,
    focused: Option<bool>,
    frontmost: Option<&str>,
    notes: &mut Vec<String>,
) -> Resolution {
    let mut memory = match deps.state.terminal_memory() {
        Ok(loaded) => loaded.into_value(),
        Err(e) => {
            notes.push(format!("terminal memory: {e}"));
            Default::default()
        }
    };
    let origin = match deps.state.focus_origin() {
        Ok(loaded) => loaded.into_value(),
        Err(e) => {
            notes.push(format!("focus origin: {e}"));
            Default::default()
        }
    };

    match terminal::learn(
        deps.config,
        &memory,
        &origin,
        &event.workspace_id,
        deps.now_ms,
        || focused,
        || frontmost.map(str::to_owned),
    ) {
        Ok(bundle_id) => {
            memory.set(&event.workspace_id, &bundle_id, deps.now_ms);
            match deps.state.save_terminal_memory(&memory) {
                Ok(()) => notes.push(format!(
                    "learned {} for workspace {}",
                    bundle_id, event.workspace_id
                )),
                Err(e) => notes.push(format!("could not save learned terminal: {e}")),
            }
        }
        Err(skip) => notes.push(format!("not learning: {skip:?}")),
    }

    terminal::resolve(deps.config, &memory, &event.workspace_id)
}

/// The job id of a notification already showing for this pane, agent and
/// status.
fn live_job<R: Runner, S: Spawner>(
    deps: &Deps<R, S>,
    record: &PaneRecord,
    event: &StatusEvent,
    agent_label: Option<&str>,
) -> Option<String> {
    let id = JobId::parse(record.live_job.as_deref()?).ok()?;
    let Ok(Loaded::Found(job)) = deps.state.job(&id) else {
        return None;
    };
    let same = job.blocks_repeat(deps.now_ms)
        && job.status == event.agent_status
        && job.agent_label.as_deref() == agent_label;
    same.then_some(job.id)
}

/// Drops every job for this pane except the one just written.
///
/// Keying this on the pane rather than on the pane record's `live_job` is
/// what handles a handover: when `claude` gives the pane up and `make`
/// claims it, the record is about a different identity and carries no job
/// id, so the old job would be left behind. The group is per-pane, so its
/// banner has already been replaced on screen — but the file would still be
/// swept later, and the sweep would then `-remove` the group this new
/// notification is using.
fn drop_other_jobs<R: Runner, S: Spawner>(
    deps: &Deps<R, S>,
    pane_id: &str,
    keep: &JobId,
    notes: &mut Vec<String>,
) {
    let ids = match deps.state.job_ids() {
        Ok(ids) => ids,
        Err(e) => {
            notes.push(format!("could not list jobs: {e}"));
            return;
        }
    };
    for id in ids {
        if &id == keep {
            continue;
        }
        let Ok(Loaded::Found(job)) = deps.state.job(&id) else {
            continue;
        };
        if job.pane_id != pane_id {
            continue;
        }
        if let Err(e) = deps.state.delete_job(&id) {
            notes.push(format!("could not delete superseded job {id}: {e}"));
        }
    }
}

/// The agent's logo for the right of the banner, if we have one.
///
/// A missing file is never an error — the banner then shows the Herdr icon
/// alone. Shell commands get no logo.
fn logo_for<R: Runner, S: Spawner>(
    deps: &Deps<R, S>,
    kind: PaneKind,
    agent_label: Option<&str>,
) -> Option<PathBuf> {
    if !deps.config.notifications.agent_logos || kind != PaneKind::Agent {
        return None;
    }
    let label = agent_label?;
    // The label reaches us from Herdr or from a shell hook and becomes part
    // of a path, so a `/` or a `..` in one must not point somewhere else.
    let plain = !label.is_empty()
        && !label.starts_with('.')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
    if !plain {
        return None;
    }
    let path = deps
        .plugin_root
        .join("icons/agents")
        .join(format!("{label}.png"));
    path.is_file().then_some(path)
}

fn save_record(state: &StateDir, record: &PaneRecord, notes: &mut Vec<String>) {
    if let Err(e) = state.save_pane_record(record) {
        notes.push(format!("could not save pane {}: {e}", record.pane_id));
    }
}
