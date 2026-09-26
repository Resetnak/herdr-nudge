//! `HERDR_PLUGIN_CONTEXT_JSON` and the `HERDR_*` environment.
//!
//! Both are read through a lookup function rather than `std::env::var`, so
//! tests can pass in a fixture's captured environment. Setting real env vars
//! from a test isn't an option: `std::env::set_var` is unsafe in edition
//! 2024 and this crate forbids unsafe.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The parts of the context JSON we use.
///
/// Two fields are left out on purpose:
///
/// - `focused_pane_id` is always the event's own pane, not where the user
///   is. It matched the event's pane in all 196 status events under
///   `tests/fixtures/raw/`, so it can't tell us whether the user is watching.
///   Ask `herdr pane get <pane>` and read its `focused` field instead.
/// - `invocation_source` is always `"api"`.
///
/// Only `workspace_id` is always present. `pane.closed` sends a much smaller
/// context with no `tab_id` and no `workspace_label`.
#[derive(Debug, Clone, Deserialize)]
pub struct Context {
    pub workspace_id: String,
    #[serde(default)]
    pub workspace_label: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
}

impl Context {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The label if there is one, otherwise the id.
    pub fn workspace_display(&self) -> &str {
        self.workspace_label
            .as_deref()
            .unwrap_or(&self.workspace_id)
    }
}

/// The paths Herdr gives a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub herdr_bin: PathBuf,
    pub state_dir: PathBuf,
    pub config_dir: PathBuf,
    pub plugin_root: PathBuf,
    /// Goes into the job file when we post a notification. The click runs
    /// later as a new process with none of this environment.
    pub socket_path: PathBuf,
}

/// Kept here so the binary and the tests use the same names.
pub const EVENT_JSON_VAR: &str = "HERDR_PLUGIN_EVENT_JSON";
pub const CONTEXT_JSON_VAR: &str = "HERDR_PLUGIN_CONTEXT_JSON";

const HERDR_BIN_PATH: &str = "HERDR_BIN_PATH";
const HERDR_PLUGIN_STATE_DIR: &str = "HERDR_PLUGIN_STATE_DIR";
const HERDR_PLUGIN_CONFIG_DIR: &str = "HERDR_PLUGIN_CONFIG_DIR";
const HERDR_PLUGIN_ROOT: &str = "HERDR_PLUGIN_ROOT";
const HERDR_SOCKET_PATH: &str = "HERDR_SOCKET_PATH";

impl Env {
    /// `lookup` reads the real environment in the binary and a fixture's env
    /// map in tests.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, MissingVars> {
        let mut missing = Vec::new();
        let mut get = |name: &'static str| match lookup(name) {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => {
                missing.push(name);
                PathBuf::new()
            }
        };

        let env = Env {
            herdr_bin: get(HERDR_BIN_PATH),
            state_dir: get(HERDR_PLUGIN_STATE_DIR),
            config_dir: get(HERDR_PLUGIN_CONFIG_DIR),
            plugin_root: get(HERDR_PLUGIN_ROOT),
            socket_path: get(HERDR_SOCKET_PATH),
        };

        if missing.is_empty() {
            Ok(env)
        } else {
            Err(MissingVars(missing))
        }
    }

    pub fn from_process() -> Result<Self, MissingVars> {
        Env::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_map(map: &BTreeMap<String, String>) -> Result<Self, MissingVars> {
        Env::from_lookup(|name| map.get(name).cloned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingVars(pub Vec<&'static str>);

impl fmt::Display for MissingVars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "missing or empty: {}", self.0.join(", "))
    }
}

impl std::error::Error for MissingVars {}

/// The plugin's directory, for a command the user runs themselves, which
/// gets no `HERDR_PLUGIN_ROOT`: the nearest directory above our binary with
/// Herdr's manifest in it. That covers `bin/`, which Herdr runs, and a
/// binary run by hand from under `target/`.
pub fn plugin_root_above(binary: &Path) -> Option<PathBuf> {
    binary
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("herdr-plugin.toml").is_file())
        .map(Path::to_path_buf)
}
