//! The parts of `doctor` that read something: `-diagnose`, Herdr's config,
//! `[agents] ignore`. Its printed layout isn't tested.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use herdr_nudge::config::Config;
use herdr_nudge::doctor::{self, toast_delivery, unknown_agents};
use herdr_nudge::herdr::Cli;
use herdr_nudge::notifier::{binary_in, bundle_path};
use herdr_nudge::state::StateDir;
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

/// Paths for `doctor::run` with nothing set up but the plugin folder.
fn inputs(dir: &Path, plugin_root: Option<PathBuf>) -> doctor::Inputs {
    doctor::Inputs {
        config_dir: Err("not set up".to_owned()),
        plugin_root,
        home: dir.to_path_buf(),
        state: StateDir::new(dir.join("state")),
        shell: Some("/bin/zsh".to_owned()),
        zshrc: dir.join(".zshrc"),
        herdr_config: dir.join("herdr-config.toml"),
    }
}

fn programs(replay: &Replay) -> Vec<String> {
    replay
        .calls
        .borrow()
        .iter()
        .map(|call| {
            Path::new(&call[0])
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// So that `-diagnose` answers for an app macOS has on record, the way a
/// post would find it.
#[test]
fn doctor_registers_the_notifier_before_diagnosing_it() {
    let dir = scratch_dir("doctor_registers");
    let root = dir.join("plugin");
    let bundle = bundle_path(&root);
    fs::create_dir_all(binary_in(&bundle).parent().unwrap()).unwrap();
    fs::write(bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();
    fs::write(binary_in(&bundle), b"#!/bin/sh\n").unwrap();
    let mut osascript = Recorded::sys("osascript-register-bundle");
    osascript.argv.truncate(5);
    osascript.argv.push(bundle.display().to_string());
    let replay = Replay::new([osascript, Recorded::sys("terminal-notifier-diagnose")]);

    let report = doctor::run(&inputs(&dir, Some(root)), &replay);

    assert_eq!(
        programs(&replay),
        ["osascript", "terminal-notifier"],
        "{}",
        report.render()
    );
    assert!(
        dir.join("state").join("registered").is_file(),
        "no marker after doctor"
    );
}

#[test]
fn doctor_warns_when_the_notifier_cant_be_registered() {
    let dir = scratch_dir("doctor_cant_register");
    let root = dir.join("plugin");
    fs::create_dir_all(bundle_path(&root)).unwrap();
    let replay = Replay::new([Recorded::sys("terminal-notifier-diagnose")]);

    let report = doctor::run(&inputs(&dir, Some(root)), &replay);

    let text = report.render();
    assert!(text.contains("can't read its Info.plist"), "{text}");
    assert_eq!(report.count(doctor::Level::Fail), 0, "{text}");
    assert_eq!(programs(&replay), ["terminal-notifier"], "{text}");
}
