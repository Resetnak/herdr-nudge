//! Puts the zsh hook where `~/.zshrc` can source it, and knows the lines
//! that do the sourcing.
//!
//! The hook is compiled into the binary, so the copy in the state directory
//! always matches the plugin that wrote it, and nothing has to find the
//! plugin's install directory. It sits next to `shell.env`, which it reads
//! for its settings. Both are rewritten on every Herdr start, so a config
//! change reaches new shells after a restart.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::config::{self, Config};
use crate::process::Runner;
use crate::state::{self, AgentsCache, Loaded, StateDir};

pub const SCRIPT: &str = include_str!("../shell/herdr-nudge.zsh");

/// Writes the hook and its `shell.env`, and returns notes for the log.
///
/// `config_dir` is `HERDR_PLUGIN_CONFIG_DIR` when the startup hook has it;
/// otherwise `herdr` is asked. With neither, the defaults are written and a
/// note says so, because a missing `shell.env` would switch the hook off.
///
/// `fetched` is the agent list `--cleanup` just got from Herdr, if it got
/// one. Without it, the list saved by an earlier run is used.
pub fn install<R: Runner>(
    state: &StateDir,
    config_dir: Option<&Path>,
    herdr_bin: Option<&Path>,
    fetched: Option<AgentsCache>,
    runner: &R,
) -> Vec<String> {
    let mut notes = Vec::new();

    let asked = match (config_dir, herdr_bin) {
        (None, Some(bin)) => match config::locate_dir(None, bin, runner) {
            Ok(dir) => {
                // Logged because the startup hook's env has had the config
                // dir every time it was checked, so asking means something
                // changed.
                notes.push(format!(
                    "no HERDR_PLUGIN_CONFIG_DIR, herdr says {}",
                    dir.display()
                ));
                Some(dir)
            }
            Err(e) => {
                notes.push(format!("could not find the config directory: {e}"));
                None
            }
        },
        _ => None,
    };
    let config_dir = config_dir.or(asked.as_deref());
    let config = match config_dir.map(Config::load) {
        Some(Ok(config)) => config,
        Some(Err(e)) => {
            notes.push(format!("{e} — shell.env gets the defaults"));
            Config::default()
        }
        None => {
            notes.push("no config directory, shell.env gets the defaults".to_owned());
            Config::default()
        }
    };

    // Without the agent list, an agent CLI run in a pane would be claimed by
    // the hook as well as reported by its own integration.
    let fetched = match fetched {
        Some(cache) => cache.agents,
        None => match state.agents_cache() {
            Ok(Loaded::Found(cache)) => cache.agents,
            _ => {
                notes.push("no agent list, the hook will time agent CLIs too".to_owned());
                BTreeSet::new()
            }
        },
    };
    let catalogue = config.agent_catalogue(fetched.iter().map(String::as_str));
    let (env, skipped) = config::shell_env(&config, &catalogue);
    for value in skipped {
        notes.push(format!(
            "left out of shell.env, not a command name: {value:?}"
        ));
    }

    let written = state::write_atomic(&state.shell_env_path(), env.as_bytes())
        .and_then(|()| state::write_atomic(&state.shell_hook_path(), SCRIPT.as_bytes()));
    match written {
        Ok(()) => notes.push(format!(
            "zsh hook written to {}",
            state.shell_hook_path().display()
        )),
        Err(e) => notes.push(format!("could not write the zsh hook: {e}")),
    }
    notes
}

/// `.zshrc`'s text, or nothing if there's no file. A byte that isn't UTF-8
/// (a Latin-1 comment, say) is replaced rather than failing the whole read.
pub fn read_zshrc(path: &Path) -> io::Result<String> {
    match fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zshrc {
    Loads,
    CommentedOut,
    OtherPath,
    Missing,
}

/// Whether `.zshrc`'s text names the hook, spelled as a path, from `~`, or
/// from `$HOME` / `${HOME}`. Only reads the file: a hook loaded from
/// somewhere else, like oh-my-zsh's custom folder, reads as `Missing`.
pub fn zshrc_loads(rc: &str, hook: &Path, home: &Path) -> Zshrc {
    let mut names = vec![hook.display().to_string()];
    if let Ok(rest) = hook.strip_prefix(home) {
        let rest = rest.display();
        names.extend([
            format!("~/{rest}"),
            format!("$HOME/{rest}"),
            format!("${{HOME}}/{rest}"),
        ]);
    }
    let names_hook = |line: &str| names.iter().any(|n| line.contains(n.as_str()));
    let (comments, code): (Vec<&str>, Vec<&str>) = rc
        .lines()
        .map(str::trim_start)
        .partition(|line| line.starts_with('#'));
    if code.iter().any(|l| names_hook(l)) {
        Zshrc::Loads
    } else if comments.iter().any(|l| names_hook(l)) {
        Zshrc::CommentedOut
    } else if code.iter().any(|l| l.contains("herdr-nudge.zsh")) {
        Zshrc::OtherPath
    } else {
        Zshrc::Missing
    }
}

/// The lines for `.zshrc`. An `if` rather than `[[ -r … ]] && source …`,
/// which leaves `$?` at 1 when the file is missing, and oh-my-zsh themes
/// show the first prompt as a failed command.
pub fn zsh_block(path: &str) -> String {
    format!("if [[ -r {path} ]]; then\n  source {path}\nfi")
}

/// `~/…` when under home and plain enough for zsh to take unquoted, the
/// full path single-quoted otherwise.
pub fn shown_path(path: &Path, home: &Path) -> String {
    let plain = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    };
    let full = path.display().to_string();
    if let Ok(rest) = path.strip_prefix(home) {
        let rest = rest.display().to_string();
        if plain(&rest) {
            return format!("~/{rest}");
        }
    }
    if plain(&full) {
        full
    } else {
        format!("'{}'", full.replace('\'', r"'\''"))
    }
}
