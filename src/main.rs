//! Dispatch, and the only place that reads the real environment.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use herdr_nudge::classify::PaneKind;
use herdr_nudge::cli::{self, Mode, ParseError};
use herdr_nudge::config::{self, Config};
use herdr_nudge::context::{self, Context, Env};
use herdr_nudge::process::System;
use herdr_nudge::state::StateDir;
use herdr_nudge::{click, doctor, event, event_summary, handler, notifier, setup_zsh, shell_hook};

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        Ok(Mode::Event) => run_event(),
        Ok(Mode::Click(id)) => run_click(&id),
        Ok(Mode::Cleanup) => run_cleanup(),
        Ok(Mode::ExampleConfig) => run_example_config(),
        Ok(Mode::Test { shell }) => run_test(shell),
        Ok(Mode::Doctor) => run_doctor(),
        Ok(Mode::SetupZsh) => run_setup_zsh(),
        Ok(Mode::Help) => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Mode::Version) => {
            println!("herdr-nudge {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
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

    let config = load_config(&env.config_dir);

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

    let report = handler::handle(&deps, &envelope, ctx.as_ref());
    for note in report.notes {
        eprintln!("herdr-nudge: {note}");
    }
    eprintln!("herdr-nudge: {:?}", report.outcome);

    ExitCode::SUCCESS
}

/// Herdr's startup hook. Exits 0 whatever happens, like an event.
///
/// The startup hook gets `HERDR_PLUGIN_STATE_DIR` and `HERDR_BIN_PATH`
/// (seen on 0.9.1). None of the variables is required: the state directory
/// has a fallback, `herdr` can say where the config is, and without `herdr`
/// the jobs still go.
fn run_cleanup() -> ExitCode {
    let Some(state) = StateDir::locate(|name| std::env::var(name).ok()) else {
        eprintln!("herdr-nudge: no HOME, cannot find the state directory");
        return ExitCode::SUCCESS;
    };
    let herdr_bin = env_path("HERDR_BIN_PATH");
    let config_dir = env_path("HERDR_PLUGIN_CONFIG_DIR");
    let system = System::default();
    let (mut notes, fetched) = handler::cleanup(
        &state,
        herdr_bin.as_deref(),
        &system,
        &system,
        herdr_nudge::state::now_ms(),
    );
    // After the cleanup, which has just refreshed the agent list the hook
    // needs.
    notes.extend(shell_hook::install(
        &state,
        config_dir.as_deref(),
        herdr_bin.as_deref(),
        fetched,
        &system,
    ));
    for note in notes {
        eprintln!("herdr-nudge: {note}");
    }
    eprintln!("herdr-nudge: --cleanup done");
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
    let home = env_path("HOME");
    let (outcome, notes) = click::run(
        &state,
        home.as_deref(),
        &system,
        &system,
        id,
        herdr_nudge::state::now_ms(),
    );
    for note in notes {
        eprintln!("herdr-nudge: {note}");
    }
    eprintln!("herdr-nudge: --click {id} {outcome:?}");
    ExitCode::SUCCESS
}

/// The config file with every setting in it, for the user to edit. Never
/// replaces one that exists: then it goes to stdout instead.
fn run_example_config() -> ExitCode {
    let system = System::default();
    let dir = match config::locate_dir(env_path("HERDR_PLUGIN_CONFIG_DIR"), &herdr_bin(), &system) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("herdr-nudge: could not find the config directory ({e}), printing instead");
            print!("{}", config::example());
            return ExitCode::SUCCESS;
        }
    };
    let path = Config::path(&dir);
    match config::write_example(&dir) {
        Ok(true) => {
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!(
                "herdr-nudge: {} exists, left alone; here is the example",
                path.display()
            );
            print!("{}", config::example());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("herdr-nudge: could not write {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

/// Run from a pane's shell, which has the pane id and the socket but none
/// of the paths a hook gets.
fn run_test(shell: bool) -> ExitCode {
    let (Some(pane_id), Some(socket_path)) =
        (env_string("HERDR_PANE_ID"), env_path("HERDR_SOCKET_PATH"))
    else {
        eprintln!(
            "herdr-nudge: test has to run inside a Herdr pane (no HERDR_PANE_ID or HERDR_SOCKET_PATH)"
        );
        return ExitCode::FAILURE;
    };
    // The shell's own environment, `XDG_STATE_HOME` included: Herdr puts
    // plugin state under it when it's set (`herdr plugin config-dir` creates
    // the directory there), and the server usually has the same one as the
    // shell it was started from. So this finds the jobs the hooks write. A
    // click, with a bare environment, can miss them; then this banner's
    // click fails the same way a real one would, rather than hiding it.
    let Some(state) = StateDir::locate(|name| std::env::var(name).ok()) else {
        eprintln!("herdr-nudge: no HOME, cannot find the state directory");
        return ExitCode::FAILURE;
    };
    let Some(self_bin) = own_path() else {
        eprintln!("herdr-nudge: cannot find my own path, nothing would be clickable");
        return ExitCode::FAILURE;
    };
    let Some(plugin_root) =
        env_path("HERDR_PLUGIN_ROOT").or_else(|| context::plugin_root_above(&self_bin))
    else {
        eprintln!(
            "herdr-nudge: no herdr-plugin.toml above {}, cannot find the notifier",
            self_bin.display()
        );
        return ExitCode::FAILURE;
    };

    let system = System::default();
    let herdr_bin = herdr_bin();
    let config = match config::locate_dir(env_path("HERDR_PLUGIN_CONFIG_DIR"), &herdr_bin, &system)
    {
        Ok(dir) => load_config(&dir),
        Err(e) => {
            eprintln!("herdr-nudge: could not find the config directory ({e}), using defaults");
            Config::default()
        }
    };

    let deps = handler::Deps {
        config: &config,
        state: &state,
        herdr_bin: &herdr_bin,
        socket_path: &socket_path,
        notifier_bin: &notifier::binary_path(&plugin_root),
        self_bin: &self_bin,
        plugin_root: &plugin_root,
        runner: &system,
        spawner: &system,
        now_ms: herdr_nudge::state::now_ms(),
        pid: std::process::id(),
    };
    let kind = if shell {
        PaneKind::Shell
    } else {
        PaneKind::Agent
    };
    let report = handler::test(&deps, &pane_id, kind);
    for note in report.notes {
        eprintln!("herdr-nudge: {note}");
    }
    match report.outcome {
        handler::Outcome::Posted(posted) => {
            println!(
                "posted \"{}\" for {pane_id}; click it from another pane or app to come back here",
                posted.title
            );
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("herdr-nudge: nothing posted: {other:?}");
            ExitCode::FAILURE
        }
    }
}

/// Run by the user, usually from a pane, which is what makes
/// [`home_and_state`] right. Herdr's config is found the way `herdr` finds it.
fn run_doctor() -> ExitCode {
    let Some((home, state)) = home_and_state() else {
        eprintln!("herdr-nudge: no HOME");
        return ExitCode::FAILURE;
    };
    let system = System::default();
    let config_dir = config::locate_dir(env_path("HERDR_PLUGIN_CONFIG_DIR"), &herdr_bin(), &system)
        .map_err(|e| e.to_string());
    let plugin_root = env_path("HERDR_PLUGIN_ROOT")
        .or_else(|| own_path().and_then(|p| context::plugin_root_above(&p)));
    // The order `herdr config check` reads them in (checked on 0.9.1).
    let herdr_config = env_path("HERDR_CONFIG_PATH").unwrap_or_else(|| {
        env_path("XDG_CONFIG_HOME")
            .unwrap_or_else(|| home.join(".config"))
            .join("herdr/config.toml")
    });
    let inputs = doctor::Inputs {
        config_dir,
        plugin_root,
        zshrc: zshrc_path(&home),
        home,
        state,
        shell: env_string("SHELL"),
        herdr_config,
    };
    let report = doctor::run(&inputs, &system);
    print!("{}", report.render());
    if report.count(doctor::Level::Fail) > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Only when the user runs it. The hook path comes from our own
/// environment, as for `doctor`, so it's right when run in a Herdr pane. Run
/// from a shell whose `XDG_STATE_HOME` differs from Herdr's server, the
/// lines name a file Herdr never writes.
fn run_setup_zsh() -> ExitCode {
    let Some((home, state)) = home_and_state() else {
        eprintln!("herdr-nudge: no HOME");
        return ExitCode::FAILURE;
    };
    let zshrc = zshrc_path(&home);
    let outcome = setup_zsh::run(
        &zshrc,
        &state.shell_hook_path(),
        &home,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    );
    match outcome {
        // The hook stays off, and it's the one case the user has to sort
        // out by hand.
        Ok(setup_zsh::Outcome::CommentedOut) => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("herdr-nudge: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `HOME`, and the state directory as our own environment finds it. In a
/// pane that environment is the server's, `XDG_STATE_HOME` included, so
/// it's where the hooks write.
fn home_and_state() -> Option<(PathBuf, StateDir)> {
    let home = env_path("HOME")?;
    let state = StateDir::locate(|name| std::env::var(name).ok())?;
    Some((home, state))
}

/// The `.zshrc` zsh reads: under `ZDOTDIR` when it's set and exported.
fn zshrc_path(home: &Path) -> PathBuf {
    env_path("ZDOTDIR")
        .unwrap_or_else(|| home.to_path_buf())
        .join(".zshrc")
}

/// A config we can't parse falls back to defaults rather than going quiet.
/// One typo must not be able to stop notifications, and defaults announce
/// themselves. The cost is that the whole file reverts, so `enabled =
/// false` goes back to true until it's fixed; `doctor` reports it.
fn load_config(dir: &Path) -> Config {
    Config::load(dir).unwrap_or_else(|err| {
        eprintln!("herdr-nudge: {err} — using defaults");
        Config::default()
    })
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// A pane's shell has `HERDR_BIN_PATH`; any other shell finds `herdr` on
/// its `PATH`.
fn herdr_bin() -> PathBuf {
    env_path("HERDR_BIN_PATH").unwrap_or_else(|| PathBuf::from("herdr"))
}

/// Our own absolute path, for the click command.
///
/// Resolved through symlinks so the path still works from a process that
/// inherits none of our environment.
fn own_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.canonicalize().unwrap_or(exe))
}
