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
//! clicked. It reads only state directories, and skips one a Herdr server
//! has put in any of those folders.
//!
//! `pgrep`, `ps`, `lsof` and `lsappinfo`, which find the terminal again, work
//! under that bare environment too (checked by hand on macOS 26).
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
use crate::state::{self, Job, Loaded, ServerStateDir, StateDir};
use crate::terminal;

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

/// `home` is the click's `HOME`, which the other state directories are
/// checked against before one is read.
pub fn run<R: Runner, S: Spawner>(
    state: &StateDir,
    home: Option<&Path>,
    runner: &R,
    spawner: &S,
    id: &JobId,
    now_ms: u64,
) -> (Outcome, Vec<String>) {
    run_with(
        state,
        |notes| state::server_state_dirs(runner, home, notes),
        runner,
        spawner,
        id,
        now_ms,
    )
}

/// [`run`], with the other places to look for the job passed in:
/// `elsewhere` is only called when the job isn't in `state`.
///
/// `state` is where the job usually is. A Herdr server started with
/// `XDG_STATE_HOME` set puts it under that instead, and the click's bare
/// environment doesn't have the variable, so then the servers are asked.
pub fn run_with<R: Runner, S: Spawner>(
    state: &StateDir,
    elsewhere: impl FnOnce(&mut Vec<String>) -> Vec<ServerStateDir>,
    runner: &R,
    spawner: &S,
    id: &JobId,
    now_ms: u64,
) -> (Outcome, Vec<String>) {
    let mut notes = Vec::new();
    let outcome = match find_job(state, elsewhere, id, &mut notes) {
        Some((dir, job)) => act(&dir, runner, spawner, job, now_ms, &mut notes),
        None => Outcome::NoJob,
    };
    (outcome, notes)
}

fn find_job(
    state: &StateDir,
    elsewhere: impl FnOnce(&mut Vec<String>) -> Vec<ServerStateDir>,
    id: &JobId,
    notes: &mut Vec<String>,
) -> Option<(StateDir, Job)> {
    match load(state, id, notes) {
        Found::Job(job) => return Some((state.clone(), *job)),
        Found::Unusable => return None,
        Found::Missing => {}
    }
    for server in elsewhere(notes) {
        if server.dir == *state {
            continue;
        }
        if server.protected {
            notes.push(format!(
                "not looking in {}, herdr server {}'s state directory: it's in a folder macOS guards",
                server.dir.root.display(),
                server.pid
            ));
            continue;
        }
        match load(&server.dir, id, notes) {
            Found::Job(job) => {
                notes.push(format!(
                    "job found in {}, herdr server {}'s state directory",
                    server.dir.root.display(),
                    server.pid
                ));
                return Some((server.dir, *job));
            }
            Found::Unusable => return None,
            Found::Missing => {}
        }
    }
    None
}

enum Found {
    Job(Box<Job>),
    Missing,
    /// There, but unreadable. Not looked for anywhere else, since it can't
    /// be anywhere else too.
    Unusable,
}

fn load(state: &StateDir, id: &JobId, notes: &mut Vec<String>) -> Found {
    match state.job(id) {
        Ok(Loaded::Found(job)) => Found::Job(Box::new(job)),
        Ok(Loaded::Missing) => Found::Missing,
        Ok(Loaded::Recovered(r)) => {
            notes.push(format!("job {id} was unreadable ({}), ignored", r.reason));
            Found::Unusable
        }
        Err(e) => {
            notes.push(format!("job {id}: {e}"));
            Found::Unusable
        }
    }
}

fn act<R: Runner, S: Spawner>(
    state: &StateDir,
    runner: &R,
    spawner: &S,
    job: Job,
    now_ms: u64,
    notes: &mut Vec<String>,
) -> Outcome {
    if job.is_expired(now_ms) {
        clear(state, spawner, &job, notes);
        return Outcome::Expired;
    }

    // Cleared before focusing, not after, so a focus that fails still leaves
    // nothing behind. The banner is gone either way, so there is nothing to
    // retry from. It also means the `pane.focused` our own focus causes
    // finds no job to withdraw.
    clear(state, spawner, &job, notes);

    // Looked for again because the notification can be an hour old, and the
    // user may have moved to another terminal since. Frontmost right now is
    // Notification Center or our own app, and neither shows a Herdr client,
    // so neither can come out of this.
    let detected = if job.detect_at_click {
        terminal::detect(runner, &job.socket_path, notes).1
    } else {
        None
    };
    let bundle_id = detected.or(job.bundle_id);
    raise_terminal(runner, bundle_id.as_deref(), notes);

    let pane_id = job.pane_id;
    match herdr::focus_pane(&job.socket_path, &pane_id, FOCUS_TIMEOUT) {
        Ok(()) => Outcome::Focused { pane_id },
        Err(e) => {
            notes.push(format!("could not focus {pane_id}: {e}"));
            Outcome::NotFocused { pane_id }
        }
    }
}

/// Brings the terminal to the front before Herdr moves focus, so the pane
/// change happens in a window the user can see.
///
/// Any failure is only noted: focusing the pane is still worth doing, and
/// the user may already be in the right window.
fn raise_terminal<R: Runner>(runner: &R, bundle_id: Option<&str>, notes: &mut Vec<String>) {
    let Some(bundle_id) = bundle_id else {
        notes.push("no terminal known, focusing without raising it".to_owned());
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
}
