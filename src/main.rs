//! Dispatch, and the only place that reads the real environment.

use std::path::PathBuf;
use std::process::ExitCode;

use herdr_nudge::cli::{self, Mode, ParseError};
use herdr_nudge::context::{self, Context, Env};
use herdr_nudge::process::System;
use herdr_nudge::state::StateDir;
use herdr_nudge::{click, event, event_summary, handler, notifier};

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        Ok(Mode::Event) => run_event(),
        Ok(Mode::Click(id)) => run_click(&id),
        // Herdr runs this as the startup hook, and a non-zero exit shows up
        // as a failed plugin. Nothing to clean up yet.
        Ok(Mode::Cleanup) => ExitCode::SUCCESS,
        Ok(Mode::Help) => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Mode::Version) => {
            println!("herdr-nudge {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(other) => {
            eprintln!("herdr-nudge: {} is not implemented yet", mode_name(&other));
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("herdr-nudge: {err}");
            eprint!("{}", cli::USAGE);
            match err {
                // A bad --click means a stale or stray notification, not a
                // bug: say so and stop.
                ParseError::BadJobId(_) => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Logs to stderr and exits 0 whatever happens, because a non-zero exit shows
/// up as a broken plugin in `herdr plugin log` and none of what can go wrong
/// here is our bug.
fn run_event() -> ExitCode {
    let env = match Env::from_process() {
        Ok(env) => env,
        Err(missing) => {
            eprintln!("herdr-nudge: incomplete environment ({missing}), nothing to do");
            return ExitCode::SUCCESS;
        }
    };

    let Ok(json) = std::env::var(context::EVENT_JSON_VAR) else {
        eprintln!("herdr-nudge: no {}, nothing to do", context::EVENT_JSON_VAR);
        return ExitCode::SUCCESS;
    };
    let envelope = match event::Envelope::parse(&json) {
        Ok(envelope) => envelope,
        Err(err) => {
            eprintln!(
                "herdr-nudge: unparseable {}: {err}",
                context::EVENT_JSON_VAR
            );
            return ExitCode::SUCCESS;
        }
    };

    let ctx = match std::env::var(context::CONTEXT_JSON_VAR) {
        Ok(json) => match Context::parse(&json) {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                eprintln!(
                    "herdr-nudge: unparseable {}: {err}",
                    context::CONTEXT_JSON_VAR
                );
                None
            }
        },
        Err(_) => None,
    };

    eprintln!("herdr-nudge: {}", event_summary(&envelope, ctx.as_ref()));

    // A config we can't parse falls back to defaults rather than going quiet.
    // One typo must not be able to stop notifications, and defaults announce
    // themselves. The cost is that the whole file reverts, so `enabled =
    // false` goes back to true until it's fixed; `doctor` reports it.
    let config = match herdr_nudge::config::Config::load(&env.config_dir) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("herdr-nudge: {err} — using defaults");
            Default::default()
        }
    };

    let self_bin = match own_path() {
        Some(path) => path,
        None => {
            eprintln!("herdr-nudge: cannot find my own path, nothing would be clickable");
            return ExitCode::SUCCESS;
        }
    };

    let state = StateDir::new(&env.state_dir);
    let system = System::default();
    let deps = handler::Deps {
        config: &config,
        state: &state,
        herdr_bin: &env.herdr_bin,
        socket_path: &env.socket_path,
        notifier_bin: &notifier::binary_path(&env.plugin_root),
        self_bin: &self_bin,
        plugin_root: &env.plugin_root,
        runner: &system,
        spawner: &system,
        now_ms: herdr_nudge::state::now_ms(),
        pid: std::process::id(),
    };

    let (swept, notes) = handler::sweep_expired(&state, &system, deps.now_ms);
    for note in notes {
        eprintln!("herdr-nudge: {note}");
    }
    if !swept.is_empty() {
        eprintln!("herdr-nudge: swept {} expired job(s)", swept.len());
    }

    let report = handler::handle(&deps, &envelope, ctx.as_ref());
    for note in report.notes {
        eprintln!("herdr-nudge: {note}");
    }
    eprintln!("herdr-nudge: {:?}", report.outcome);

    ExitCode::SUCCESS
}

/// The click has no `HERDR_*` environment, so it finds the state directory
/// itself and reads everything else out of the job file.
fn run_click(id: &cli::JobId) -> ExitCode {
    let Some(state) = StateDir::locate(|name| std::env::var(name).ok()) else {
        eprintln!("herdr-nudge: no HOME, cannot find the state directory");
        return ExitCode::SUCCESS;
    };

    let system = System::default();
    let (outcome, notes) = click::run(&state, &system, &system, id, herdr_nudge::state::now_ms());
    for note in notes {
        eprintln!("herdr-nudge: {note}");
    }
    eprintln!("herdr-nudge: --click {id} {outcome:?}");
    ExitCode::SUCCESS
}

/// Our own absolute path, for the click command.
///
/// Resolved through symlinks so the path still works from a process that
/// inherits none of our environment.
fn own_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.canonicalize().unwrap_or(exe))
}

fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Event => "event mode",
        Mode::Click(_) => "--click",
        Mode::Cleanup => "--cleanup",
        Mode::Doctor => "doctor",
        Mode::Bind(_) => "bind",
        Mode::Test { .. } => "test",
        Mode::Help => "--help",
        Mode::Version => "--version",
    }
}
