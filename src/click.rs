//! What happens when a notification is clicked.
//!
//! macOS relaunches our notifier bundle and runs the command stored on the
//! clicked notification, which is `'<our binary>' --click <job id>`. That
//! process gets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME`, `USER`, `SHELL`
//! and `TMPDIR`, and nothing else — no `HERDR_*` at all. So everything comes
//! from the job file.
//!
//! It must also stay out of `~/Documents`, `~/Downloads` and `~/Desktop`.
//! Anything it reads there would put up a permission prompt under the Herdr
//! Nudge name, which the user has no way to connect to what they just
//! clicked. It touches the state directory and nothing else.
//!
//! Bringing the pane forward takes two steps: `open -b` raises the terminal,
//! then the Herdr socket's `pane.focus` moves to the pane. `herdr agent
//! focus` would be simpler but fails on a plain shell pane.

use std::path::Path;
use std::time::Duration;

use crate::cli::JobId;
use crate::herdr;
use crate::notifier::Notifier;
use crate::process::{Runner, Spawner};
use crate::state::{Job, Loaded, StateDir};

const OPEN: &str = "/usr/bin/open";

/// A click is the user waiting on us, but a pane that isn't focused within
/// this long isn't going to be.
const FOCUS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Already clicked, already swept, or from an older install. Not an
    /// error: a notification can sit in Notification Center for an hour.
    NoJob,
    Expired,
    Focused {
        pane_id: String,
    },
    /// Herdr didn't focus the pane, most often because it has closed since.
    /// The notes say why.
    NotFocused {
        pane_id: String,
    },
}

pub fn run<R: Runner, S: Spawner>(
    state: &StateDir,
    runner: &R,
    spawner: &S,
    id: &JobId,
    now_ms: u64,
) -> (Outcome, Vec<String>) {
    let mut notes = Vec::new();

    let job = match state.job(id) {
        Ok(Loaded::Found(job)) => job,
        Ok(Loaded::Missing) => return (Outcome::NoJob, notes),
        Ok(Loaded::Recovered(r)) => {
            notes.push(format!("job {id} was unreadable ({}), ignored", r.reason));
            return (Outcome::NoJob, notes);
        }
        Err(e) => {
            notes.push(format!("job {id}: {e}"));
            return (Outcome::NoJob, notes);
        }
    };

    if job.is_expired(now_ms) {
        clear(state, spawner, &job, &mut notes);
        return (Outcome::Expired, notes);
    }

    // Cleared before focusing, not after. Our own `pane.focus` makes Herdr
    // send `pane.focused`, which runs the event hook for this same pane. Once
    // that hook tidies up live jobs, clearing first leaves it nothing to race
    // us for.
    clear(state, spawner, &job, &mut notes);

    mark_focus_origin(state, &job.workspace_id, now_ms, &mut notes);
    raise_terminal(runner, job.bundle_id.as_deref(), &mut notes);

    let pane_id = job.pane_id;
    match herdr::focus_pane(&job.socket_path, &pane_id, FOCUS_TIMEOUT) {
        Ok(()) => (Outcome::Focused { pane_id }, notes),
        Err(e) => {
            notes.push(format!("could not focus {pane_id}: {e}"));
            (Outcome::NotFocused { pane_id }, notes)
        }
    }
}

/// Stops the terminal we are about to raise from being learned as this
/// workspace's. Notification Center can still be frontmost for a moment
/// after the click, and whatever `open -b` raises is our choice, not
/// evidence of where the user keeps this workspace.
fn mark_focus_origin(state: &StateDir, workspace_id: &str, now_ms: u64, notes: &mut Vec<String>) {
    let mut origin = match state.focus_origin() {
        Ok(loaded) => loaded.into_value(),
        Err(e) => {
            notes.push(format!("focus origin: {e}"));
            Default::default()
        }
    };
    origin.mark(workspace_id, now_ms);
    if let Err(e) = state.save_focus_origin(&origin) {
        notes.push(format!("could not write focus origin: {e}"));
    }
}

/// Brings the terminal to the front before Herdr moves focus, so the pane
/// change happens in a window the user can see.
///
/// Any failure is only noted: focusing the pane is still worth doing, and
/// the user may already be in the right window.
fn raise_terminal<R: Runner>(runner: &R, bundle_id: Option<&str>, notes: &mut Vec<String>) {
    let Some(bundle_id) = bundle_id else {
        notes.push("no terminal known for this workspace, focusing without raising it".to_owned());
        return;
    };
    match runner.run(Path::new(OPEN), &["-b", bundle_id]) {
        Ok(out) if out.success() => {}
        Ok(out) => notes.push(format!(
            "open -b {bundle_id} exited {:?}: {}",
            out.code,
            out.stderr.trim()
        )),
        Err(e) => notes.push(format!("open -b {bundle_id}: {e}")),
    }
}

/// Withdraws the notification and forgets the job.
///
/// The notification is dismissed by the click itself, but a group can hold
/// more than the one banner, and `-remove` is what clears Notification
/// Center.
fn clear<S: Spawner>(state: &StateDir, spawner: &S, job: &Job, notes: &mut Vec<String>) {
    let notifier = Notifier {
        binary: &job.notifier_path,
        spawner,
    };
    if let Err(e) = notifier.remove(&job.group) {
        notes.push(format!("could not remove group {}: {e}", job.group));
    }

    if let Ok(id) = JobId::parse(&job.id)
        && let Err(e) = state.delete_job(&id)
    {
        notes.push(format!("could not delete job {}: {e}", job.id));
    }

    // Leaves the classification behind; only the notification is gone.
    match state.pane_record(&job.pane_id) {
        Ok(Loaded::Found(mut record)) if record.live_job.as_deref() == Some(job.id.as_str()) => {
            record.live_job = None;
            if let Err(e) = state.save_pane_record(&record) {
                notes.push(format!("could not update pane {}: {e}", job.pane_id));
            }
        }
        _ => {}
    }
}
