//! Puts the zsh hook where `~/.zshrc` can source it.
//!
//! The hook is compiled into the binary, so the copy in the state directory
//! always matches the plugin that wrote it, and nothing has to find the
//! plugin's install directory. It sits next to `shell.env`, which it reads
//! for its settings. Both are rewritten on every Herdr start, so a config
//! change reaches new shells after a restart.

use std::collections::BTreeSet;
use std::path::Path;

use crate::config::{self, Config};
use crate::herdr::Cli;
use crate::process::Runner;
use crate::state::{self, AgentsCache, Loaded, PLUGIN_ID, StateDir};

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
        (None, Some(bin)) => match (Cli { bin, runner }).plugin_config_dir(PLUGIN_ID) {
            Ok(dir) => {
                // Logged because nobody has yet seen whether the startup
                // hook's env has the config dir.
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
