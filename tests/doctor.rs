//! The parts of `doctor` that read something: `-diagnose`, Herdr's config,
//! `[agents] ignore`. Its printed layout isn't tested.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use herdr_nudge::config::Config;
use herdr_nudge::doctor::{self, toast_delivery, unknown_agents};
use herdr_nudge::herdr::Cli;
use support::{Recorded, Replay, scratch_dir};

fn cli(replay: &Replay) -> Cli<'_, Replay> {
    Cli {
        bin: Path::new("/usr/local/bin/herdr"),
        runner: replay,
    }
}

fn manifests() -> BTreeSet<String> {
    let replay = Replay::new([Recorded::cli("agent-manifests")]);
    cli(&replay)
        .agent_manifests()
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn the_diagnosis_of_a_working_bundle() {
    let found = doctor::diagnose(&Recorded::sys("terminal-notifier-diagnose").stdout);
    assert_eq!(found.authorization.as_deref(), Some("authorized"));
    assert_eq!(found.alert_style.as_deref(), Some("banners"));
}

#[test]
fn toast_delivery_is_read_from_herdrs_config() {
    let dir = scratch_dir("doctor_toast");
    let path = dir.join("config.toml");
    assert_eq!(toast_delivery(&path), None, "no file");

    fs::write(
        &path,
        "[ui.toast]\ndelivery = \"system\"\n\n[ui.sound]\nenabled = true\n",
    )
    .unwrap();
    assert_eq!(toast_delivery(&path).as_deref(), Some("system"));

    fs::write(&path, "[ui]\nstatus_indicators = \"dots\"\n").unwrap();
    assert_eq!(
        toast_delivery(&path),
        None,
        "not set means Herdr's default, off"
    );
}

#[test]
fn an_ignored_agent_must_be_a_label_herdr_uses() {
    let mut config = Config::default();
    config.agents.ignore = vec!["Codex".into(), "codex".into(), "omp".into()];
    assert_eq!(unknown_agents(&config, &manifests()), vec!["Codex"]);
}

/// `known_agents_remove` makes a pane a shell command, but the label is
/// still Herdr's, and `[agents] ignore` still applies to it as an agent.
#[test]
fn a_removed_agent_is_still_a_label_herdr_uses() {
    let mut config = Config::default();
    config.agents.ignore = vec!["codex".into(), "mine".into()];
    config.shell.known_agents_remove = vec!["codex".into()];
    config.shell.known_agents_extra = vec!["mine".into()];
    assert!(unknown_agents(&config, &manifests()).is_empty());
}
