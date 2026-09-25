//! `herdr-nudge doctor`: the setup problems that make banners go missing,
//! and how to fix each.
//!
//! Each check notes what it found and moves on. A `Fail` stops banners from
//! showing, and makes the exit code non-zero.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::herdr;
use crate::notifier;
use crate::process::Runner;
use crate::state::{self, Loaded, StateDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Note,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<Line>,
}

impl Report {
    fn add(&mut self, level: Level, text: impl Into<String>) {
        self.lines.push(Line {
            level,
            text: text.into(),
        });
    }

    pub fn count(&self, level: Level) -> usize {
        self.lines.iter().filter(|l| l.level == level).count()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let tag = match line.level {
                Level::Ok => "ok",
                Level::Note => "info",
                Level::Warn => "warn",
                Level::Fail => "FAIL",
            };
            let mut text = line.text.lines();
            out.push_str(&format!("{tag:<5} {}\n", text.next().unwrap_or("")));
            for more in text {
                out.push_str(&format!("      {more}\n"));
            }
        }
        let (fails, warns) = (self.count(Level::Fail), self.count(Level::Warn));
        if fails + warns == 0 {
            out.push_str("\nNo problems found.\n");
        } else {
            out.push_str(&format!("\n{fails} problem(s), {warns} warning(s).\n"));
        }
        out
    }
}

/// What `doctor` needs from the environment it runs in, usually a pane's
/// shell, which has none of the plugin paths a hook gets.
pub struct Inputs {
    /// Why it couldn't be found, if it couldn't.
    pub config_dir: Result<PathBuf, String>,
    pub plugin_root: Option<PathBuf>,
    pub home: PathBuf,
    /// [`StateDir::locate`] over doctor's own environment. A pane's shell
    /// inherits its server's, so this is where the hooks write, even under
    /// `XDG_STATE_HOME`.
    pub state: StateDir,
    pub shell: Option<String>,
    pub zshrc: PathBuf,
    /// Herdr's own `config.toml`, for the `[ui.toast]` check.
    pub herdr_config: PathBuf,
}

pub fn run(inputs: &Inputs, runner: &impl Runner) -> Report {
    let mut report = Report::default();
    let manifests = match inputs.state.agents_cache() {
        Ok(Loaded::Found(cache)) => Some(cache.agents),
        _ => None,
    };
    check_state(&mut report, inputs);
    check_config(&mut report, &inputs.config_dir, manifests.as_ref());
    check_notifier(&mut report, runner, inputs.plugin_root.as_deref());
    check_toast(&mut report, &inputs.herdr_config);
    check_zsh(&mut report, inputs);
    report
}

/// A click won't read a state directory in ~/Documents and the like, so
/// with the hooks writing there, no click can find its notification.
fn check_state(report: &mut Report, inputs: &Inputs) {
    if state::in_protected_folder(&inputs.state.root, &inputs.home) {
        report.add(
            Level::Fail,
            format!(
                "Herdr keeps this plugin's state in {}, a folder macOS guards,\n\
                 so clicking a notification does nothing. Set XDG_STATE_HOME to a folder\n\
                 outside ~/Documents, ~/Downloads and ~/Desktop, then restart Herdr.",
                inputs.state.root.display()
            ),
        );
    }
}

fn check_config(
    report: &mut Report,
    config_dir: &Result<PathBuf, String>,
    manifests: Option<&BTreeSet<String>>,
) {
    let dir = match config_dir {
        Ok(dir) => dir,
        Err(e) => {
            report.add(
                Level::Warn,
                format!("can't find the config directory ({e}), so can't check config.toml"),
            );
            return;
        }
    };
    let path = Config::path(dir);
    if !path.exists() {
        report.add(
            Level::Note,
            format!(
                "no {}, so every setting is at its default.\n\
                 `herdr-nudge example-config` writes one with every setting in it.",
                path.display()
            ),
        );
        return;
    }
    let config = match Config::load(dir) {
        Ok(config) => config,
        Err(e) => {
            report.add(
                Level::Fail,
                format!("{e}\nUntil it's fixed, every setting is at its default."),
            );
            return;
        }
    };
    report.add(Level::Ok, format!("settings: {}", path.display()));
    if let Some(manifests) = manifests {
        for label in unknown_agents(&config, manifests) {
            report.add(
                Level::Warn,
                format!(
                    "[agents] ignore has {label:?}, which isn't an agent label Herdr uses, so it\n\
                     probably mutes nothing. Labels are lowercase, like \"claude\" or \"codex\"."
                ),
            );
        }
    }
}

/// `[agents] ignore` entries that aren't an agent label Herdr uses, or one
/// `known_agents_extra` adds. The match is exact, so `"Codex"` or a display
/// name like `"Claude Code"` mutes nothing. `manifests` is the list as Herdr
/// gave it: `known_agents_remove` doesn't make a label unknown.
pub fn unknown_agents<'a>(config: &'a Config, manifests: &BTreeSet<String>) -> Vec<&'a str> {
    let known = |label: &str| {
        manifests.contains(label)
            || herdr::AGENTS_WITHOUT_MANIFEST.contains(&label)
            || config.shell.known_agents_extra.iter().any(|e| e == label)
    };
    config
        .agents
        .ignore
        .iter()
        .map(String::as_str)
        .filter(|label| !known(label))
        .collect()
}

fn check_notifier(report: &mut Report, runner: &impl Runner, root: Option<&Path>) {
    let Some(root) = root else {
        report.add(
            Level::Fail,
            "can't find the plugin's directory (no herdr-plugin.toml above this program)",
        );
        return;
    };
    let binary = notifier::binary_path(root);
    let out = match runner.run(&binary, &["-diagnose"]) {
        Ok(out) => out,
        Err(e) => {
            report.add(Level::Fail, format!("can't run {}: {e}", binary.display()));
            return;
        }
    };
    let found = diagnose(&out.stdout);
    let settings = "System Settings > Notifications > Herdr Nudge";
    match found.authorization.as_deref() {
        Some("authorized" | "provisional") => {
            report.add(Level::Ok, "macOS allows Herdr Nudge's notifications")
        }
        Some("not requested yet") => report.add(
            Level::Warn,
            "macOS hasn't asked about Herdr Nudge yet. Run `herdr-nudge test` in a pane and allow it.",
        ),
        Some("denied") => report.add(
            Level::Fail,
            format!("notifications are turned off for Herdr Nudge: {settings}"),
        ),
        Some(other) => report.add(Level::Warn, format!("authorization: {other}")),
        None => report.add(
            Level::Fail,
            format!(
                "{} -diagnose didn't report (exit {:?}): {}",
                binary.display(),
                out.code,
                out.stderr.trim()
            ),
        ),
    }
    match found.alert_style.as_deref() {
        Some("banners") => report.add(
            Level::Note,
            format!(
                "banners slide away after a few seconds and wait in Notification Center.\n\
                 To keep them on screen, choose Alerts in {settings}."
            ),
        ),
        Some("none") => report.add(
            Level::Fail,
            format!("alert style is None, so nothing shows on screen: {settings}"),
        ),
        _ => {}
    }
}

/// The parts of `terminal-notifier -diagnose` we act on
/// (`tests/fixtures/sys/terminal-notifier-diagnose.json`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    pub authorization: Option<String>,
    pub alert_style: Option<String>,
}

pub fn diagnose(stdout: &str) -> Diagnosis {
    let mut found = Diagnosis::default();
    for line in stdout.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("authorization") {
            found.authorization = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("alert style") {
            found.alert_style = Some(value.trim().to_lowercase());
        }
    }
    found
}

fn check_toast(report: &mut Report, herdr_config: &Path) {
    if let Some(delivery) = toast_delivery(herdr_config)
        && (delivery == "system" || delivery == "terminal")
    {
        report.add(
            Level::Warn,
            format!(
                "[ui.toast] delivery = \"{delivery}\" in {}: Herdr posts its own notification\n\
                 for the same events, and a click on that one can't take you to the pane.\n\
                 Set it to \"herdr\" or \"off\".",
                herdr_config.display()
            ),
        );
    }
}

/// `[ui.toast] delivery` in Herdr's config, if it's set. Herdr's default is
/// `off` (`herdr --default-config`).
pub fn toast_delivery(herdr_config: &Path) -> Option<String> {
    let text = fs::read_to_string(herdr_config).ok()?;
    let value: toml::Table = toml::from_str(&text).ok()?;
    value
        .get("ui")?
        .get("toast")?
        .get("delivery")?
        .as_str()
        .map(str::to_owned)
}

fn check_zsh(report: &mut Report, inputs: &Inputs) {
    if !inputs.shell.as_deref().is_some_and(|s| s.ends_with("/zsh")) {
        report.add(
            Level::Note,
            "your shell isn't zsh, so long shell commands won't notify. Agents still do.",
        );
        return;
    }
    let hook = inputs.state.shell_hook_path();
    if !hook.is_file() {
        report.add(
            Level::Warn,
            format!(
                "{} isn't written yet. Herdr writes it when it starts; restart Herdr.",
                hook.display()
            ),
        );
    }

    let rc = fs::read_to_string(&inputs.zshrc).unwrap_or_default();
    let shown = shown_path(&hook, &inputs.home);
    let rc_name = shown_path(&inputs.zshrc, &inputs.home);
    match zshrc_loads(&rc, &hook, &inputs.home) {
        Zshrc::Loads => report.add(Level::Ok, format!("{rc_name} loads the zsh hook")),
        Zshrc::CommentedOut => report.add(
            Level::Warn,
            format!("{rc_name} has the zsh hook, but commented out"),
        ),
        Zshrc::OtherPath => report.add(
            Level::Warn,
            format!("{rc_name} loads a herdr-nudge.zsh, but not {shown}"),
        ),
        Zshrc::Missing => report.add(
            Level::Note,
            format!(
                "{rc_name} doesn't load the zsh hook. If you load it some other way, ignore\n\
                 this. Otherwise, to get a notification when a long command finishes, add\n\
                 this to {rc_name} and open a new shell:\n\n{}",
                zsh_block(&shown)
            ),
        ),
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
