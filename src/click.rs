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
//! Bringing the pane forward is not written yet; see `run`.

use crate::cli::JobId;
use crate::notifier::Notifier;
use crate::process::Spawner;
use crate::state::{Job, Loaded, StateDir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Already clicked, already swept, or from an older install. Not an
    /// error: a notification can sit in Notification Center for an hour.
    NoJob,
    Expired,
    /// The job was read and cleared. Focusing the pane comes later.
    Cleared {
        pane_id: String,
    },
}

pub fn run<S: Spawner>(
    state: &StateDir,
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

    // Focusing the pane goes here: write the focus-origin marker so the
    // terminal we are about to raise isn't learned as this workspace's, run
    // `open -b` if the job names a bundle, then ask the Herdr socket for
    // `pane.focus`. Not written yet, so a click currently only tidies up.

    clear(state, spawner, &job, &mut notes);
    (
        Outcome::Cleared {
            pane_id: job.pane_id,
        },
        notes,
    )
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
