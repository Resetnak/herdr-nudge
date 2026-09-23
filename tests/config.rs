//! `config.toml` and the `shell.env` generated from it.

mod support;

use std::collections::BTreeSet;
use std::fs;

use herdr_nudge::config::{Config, shell_env};
use herdr_nudge::event::AgentStatus;
use herdr_nudge::herdr::Cli;
use support::{Recorded, Replay, scratch_dir};

#[test]
fn no_file_means_defaults() {
    let dir = scratch_dir("no_file_means_defaults");
    assert_eq!(Config::load(&dir).unwrap(), Config::default());
}

#[test]
fn an_empty_file_means_defaults() {
    assert_eq!(Config::parse("").unwrap(), Config::default());
}

#[test]
fn defaults() {
    let c = Config::default();
    assert_eq!(c.default_terminal, None);
    assert_eq!(c.notifications.clickable_secs, 3600);
    assert!(c.notifications.sound && c.notifications.agent_logos);
    assert!(c.agents.enabled && c.shell.enabled);
    assert_eq!(c.agents.statuses, [AgentStatus::Blocked, AgentStatus::Done]);
    assert_eq!(c.shell.statuses, [AgentStatus::Idle, AgentStatus::Done]);
    assert_eq!(c.shell.min_seconds, 30);
    assert!(!c.shell.notify_on_failure_only);
}

#[test]
fn a_full_config_parses() {
    let c = Config::parse(
        r#"
default_terminal = "com.mitchellh.ghostty"

[notifications]
clickable_secs = 600
sound = false
agent_logos = false

[agents]
enabled = true
statuses = ["blocked"]

[shell]
enabled = false
min_seconds = 5
statuses = ["done"]
notify_on_failure_only = true
ignore_commands = ["vim"]
known_agents_extra = ["aider"]
known_agents_remove = ["pi"]
"#,
    )
    .unwrap();

    assert_eq!(c.default_terminal.as_deref(), Some("com.mitchellh.ghostty"));
    assert_eq!(c.notifications.clickable_secs, 600);
    assert!(!c.notifications.sound);
    assert_eq!(c.agents.statuses, [AgentStatus::Blocked]);
    assert!(!c.shell.enabled);
    assert_eq!(c.shell.statuses, [AgentStatus::Done]);
    assert_eq!(c.shell.known_agents_extra, ["aider"]);
}

#[test]
fn a_partial_section_keeps_the_other_defaults() {
    let c = Config::parse("[shell]\nmin_seconds = 90\n").unwrap();
    assert_eq!(c.shell.min_seconds, 90);
    assert_eq!(c.shell.statuses, Config::default().shell.statuses);
    assert_eq!(
        c.shell.ignore_commands,
        Config::default().shell.ignore_commands
    );
}

#[test]
fn a_misspelled_key_is_an_error() {
    let err = Config::parse("defualt_terminal = \"com.mitchellh.ghostty\"\n").unwrap_err();
    assert!(err.contains("defualt_terminal"), "{err}");
    let err = Config::parse("[shell]\nmin_second = 5\n").unwrap_err();
    assert!(err.contains("min_second"), "{err}");
}

#[test]
fn an_unknown_status_is_an_error_not_unknown() {
    let err = Config::parse("[agents]\nstatuses = [\"blokced\"]\n").unwrap_err();
    assert!(err.contains("blokced"), "{err}");
    // "unknown" is a real Herdr status but never a useful trigger.
    assert!(Config::parse("[agents]\nstatuses = [\"unknown\"]\n").is_err());
}

#[test]
fn an_empty_bundle_id_is_an_error() {
    assert!(Config::parse("default_terminal = \"\"\n").is_err());
}

#[test]
fn a_load_error_names_the_file() {
    let dir = scratch_dir("a_load_error_names_the_file");
    fs::write(dir.join("config.toml"), "default_terminal = \n").unwrap();
    let err = Config::load(&dir).unwrap_err();
    assert!(
        err.to_string()
            .starts_with(&dir.join("config.toml").display().to_string())
    );
}

fn fetched_catalogue() -> Vec<String> {
    let replay = Replay::new([Recorded::cli("agent-manifests")]);
    let cli = Cli {
        bin: "/Users/dev/.local/bin/herdr".as_ref(),
        runner: &replay,
    };
    cli.agent_manifests().unwrap()
}

#[test]
fn the_catalogue_applies_extra_then_remove() {
    let config = Config::parse(
        "[shell]\nknown_agents_extra = [\"aider\", \"both\"]\nknown_agents_remove = [\"pi\", \"both\"]\n",
    )
    .unwrap();
    let fetched = fetched_catalogue();
    let catalogue = config.agent_catalogue(fetched.iter().map(String::as_str));

    assert!(catalogue.contains("claude"));
    assert!(catalogue.contains("aider"), "known_agents_extra not added");
    assert!(!catalogue.contains("pi"), "known_agents_remove not removed");
    assert!(!catalogue.contains("both"), "remove should win over extra");
    assert_eq!(catalogue.len(), fetched.len());
}

#[test]
fn shell_env_lists_settings_ignores_and_agents() {
    let config =
        Config::parse("[shell]\nmin_seconds = 45\nignore_commands = [\"vim\", \"less\"]\n")
            .unwrap();
    let catalogue: BTreeSet<String> = ["claude", "codex"].map(String::from).into();
    let (text, skipped) = shell_env(&config, &catalogue);

    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(
        lines,
        [
            "enabled=1",
            "min_seconds=45",
            "ignore=vim",
            "ignore=less",
            "agent=claude",
            "agent=codex"
        ]
    );
    assert!(skipped.is_empty());
}

#[test]
fn shell_env_drops_values_that_would_break_a_line() {
    let config = Config::parse(
        "[shell]\nenabled = false\nignore_commands = [\"ok\", \"two words\", \"new\\nline\", \"\", \"tab\\there\"]\n",
    )
    .unwrap();
    let (text, skipped) = shell_env(&config, &BTreeSet::new());

    assert!(text.contains("enabled=0\n"));
    assert!(text.contains("ignore=ok\n"));
    assert_eq!(text.lines().filter(|l| l.starts_with("ignore=")).count(), 1);
    assert_eq!(skipped, ["two words", "new\nline", "", "tab\there"]);
}
