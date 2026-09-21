//! `config.toml` in `$HERDR_PLUGIN_CONFIG_DIR`, and the `shell.env` file the
//! zsh hook reads.
//!
//! A missing file means all defaults. Unknown keys are an error rather than
//! ignored, so a typo like `defualt_terminal` shows up instead of silently
//! doing nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::event::AgentStatus;

pub const FILE_NAME: &str = "config.toml";

/// The terminals Herdr 0.9.0 itself recognises (hardcoded in its macOS
/// platform code). Starting from the same list means we only learn a
/// terminal Herdr would also call one.
pub const DEFAULT_TERMINAL_ALLOWLIST: [&str; 6] = [
    "com.apple.Terminal",
    "com.github.wez.wezterm",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub default_terminal: Option<String>,
    /// Replaces the default list rather than adding to it.
    pub terminal_allowlist: Vec<String>,
    pub notifications: Notifications,
    pub agents: Agents,
    pub shell: Shell,
    /// Workspace id to bundle id, e.g. `w8 = "com.mitchellh.ghostty"`.
    pub workspaces: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notifications {
    /// How long a notification can still be clicked from Notification
    /// Center. How long a banner stays on screen is the user's macOS
    /// Banners/Alerts setting, not something we control.
    pub clickable_secs: u64,
    pub sound: bool,
    pub agent_logos: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agents {
    pub enabled: bool,
    #[serde(deserialize_with = "statuses")]
    pub statuses: Vec<AgentStatus>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shell {
    pub enabled: bool,
    pub min_seconds: u64,
    /// `done` is in the default because Herdr turns a reported `idle` into
    /// `done` when the user wasn't watching the pane, and that's the case
    /// worth a notification. See `tests/fixtures/events/shell/done-unwatched-failed.json`.
    #[serde(deserialize_with = "statuses")]
    pub statuses: Vec<AgentStatus>,
    pub notify_on_failure_only: bool,
    pub ignore_commands: Vec<String>,
    pub ignore_agents: Vec<String>,
    pub known_agents_extra: Vec<String>,
    pub known_agents_remove: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_terminal: None,
            terminal_allowlist: DEFAULT_TERMINAL_ALLOWLIST
                .iter()
                .map(|s| s.to_string())
                .collect(),
            notifications: Notifications::default(),
            agents: Agents::default(),
            shell: Shell::default(),
            workspaces: BTreeMap::new(),
        }
    }
}

impl Default for Notifications {
    fn default() -> Self {
        Notifications {
            clickable_secs: 3600,
            sound: true,
            agent_logos: true,
        }
    }
}

impl Default for Agents {
    fn default() -> Self {
        Agents {
            enabled: true,
            statuses: vec![AgentStatus::Blocked, AgentStatus::Done],
        }
    }
}

impl Default for Shell {
    fn default() -> Self {
        Shell {
            enabled: true,
            min_seconds: 30,
            statuses: vec![AgentStatus::Idle, AgentStatus::Done],
            notify_on_failure_only: false,
            ignore_commands: ["vim", "nvim", "less", "man", "ssh", "top", "htop"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ignore_agents: Vec::new(),
            known_agents_extra: Vec::new(),
            known_agents_remove: Vec::new(),
        }
    }
}

/// Only real statuses. `AgentStatus` itself turns any unknown word into
/// `Unknown` so a new Herdr status can't break event parsing, but in the
/// config an unknown word is a typo, and should say so.
fn statuses<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<AgentStatus>, D::Error> {
    let words = Vec::<String>::deserialize(deserializer)?;
    words
        .iter()
        .map(|word| match word.as_str() {
            "idle" => Ok(AgentStatus::Idle),
            "working" => Ok(AgentStatus::Working),
            "blocked" => Ok(AgentStatus::Blocked),
            "done" => Ok(AgentStatus::Done),
            other => Err(serde::de::Error::custom(format!(
                "unknown status {other:?}, expected idle, working, blocked or done"
            ))),
        })
        .collect()
}

#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join(FILE_NAME)
    }

    /// Defaults if there's no file.
    pub fn load(config_dir: &Path) -> Result<Config, ConfigError> {
        let path = Config::path(config_dir);
        let error = |message: String| ConfigError {
            path: path.clone(),
            message,
        };
        match fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text).map_err(error),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(error(e.to_string())),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.check()?;
        Ok(config)
    }

    /// An empty bundle id would reach `open -b ""` on a click.
    fn check(&self) -> Result<(), String> {
        if self.default_terminal.as_deref().is_some_and(str::is_empty) {
            return Err("default_terminal is empty".to_owned());
        }
        for (workspace, bundle_id) in &self.workspaces {
            if bundle_id.is_empty() {
                return Err(format!("[workspaces] {workspace} is empty"));
            }
        }
        Ok(())
    }

    /// Herdr's agent catalogue with our config applied on top: `extra`
    /// added, then `remove` taken out. So a label in both lists ends up
    /// removed.
    pub fn agent_catalogue<'a>(
        &self,
        fetched: impl IntoIterator<Item = &'a str>,
    ) -> BTreeSet<String> {
        let mut catalogue: BTreeSet<String> = fetched.into_iter().map(str::to_owned).collect();
        catalogue.extend(self.shell.known_agents_extra.iter().cloned());
        for label in &self.shell.known_agents_remove {
            catalogue.remove(label);
        }
        catalogue
    }
}

/// The settings the zsh hook needs, as `key=value` lines.
///
/// Not shell syntax: the hook has to split each line on the first `=` and
/// must never source or eval the file, or a value could run code. Command
/// names are compared word by word, so an entry with whitespace or a
/// control character could never match and would only break the line
/// format. Those are left out and returned so the caller can log them.
pub fn shell_env(config: &Config, catalogue: &BTreeSet<String>) -> (String, Vec<String>) {
    let mut out =
        String::from("# Written by herdr-nudge from config.toml. Edits are overwritten.\n");
    out.push_str(&format!("enabled={}\n", u8::from(config.shell.enabled)));
    out.push_str(&format!("min_seconds={}\n", config.shell.min_seconds));

    let mut skipped = Vec::new();
    let mut list = |key: &str, values: &mut dyn Iterator<Item = &String>| {
        for value in values {
            if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
                skipped.push(value.clone());
            } else {
                out.push_str(&format!("{key}={value}\n"));
            }
        }
    };
    list("ignore", &mut config.shell.ignore_commands.iter());
    list("agent", &mut catalogue.iter());

    (out, skipped)
}
