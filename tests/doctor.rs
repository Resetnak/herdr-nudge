//! The parts of `doctor` that read something: `-diagnose`, Herdr's config,
//! `[agents] ignore`, the `.zshrc` lines. Its printed layout isn't tested.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use herdr_nudge::config::Config;
use herdr_nudge::doctor::{self, Zshrc, shown_path, toast_delivery, unknown_agents, zshrc_loads};
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

#[test]
fn zshrc_is_read_for_the_hook_however_its_path_is_written() {
    let home = Path::new("/Users/dev");
    let hook = Path::new("/Users/dev/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh");
    let rest = ".local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh";
    for line in [
        format!("source ~/{rest}"),
        format!("  source $HOME/{rest}"),
        format!("[[ -r ${{HOME}}/{rest} ]] && source ${{HOME}}/{rest}"),
        format!("source /Users/dev/{rest}"),
    ] {
        assert_eq!(zshrc_loads(&line, hook, home), Zshrc::Loads, "{line}");
    }
    assert_eq!(
        zshrc_loads(&format!("export A=1\n  # source ~/{rest}\n"), hook, home),
        Zshrc::CommentedOut
    );
    assert_eq!(
        zshrc_loads("source ~/elsewhere/herdr-nudge.zsh\n", hook, home),
        Zshrc::OtherPath
    );
    assert_eq!(zshrc_loads("plugins=(git)\n", hook, home), Zshrc::Missing);
}

#[test]
fn zshrc_paths_are_shown_the_way_zsh_takes_them() {
    let home = Path::new("/Users/dev");
    let hook = PathBuf::from("/Users/dev/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh");
    assert_eq!(
        shown_path(&hook, home),
        "~/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh"
    );
    assert_eq!(
        shown_path(Path::new("/Volumes/My Disk/it's.zsh"), home),
        r"'/Volumes/My Disk/it'\''s.zsh'"
    );
    assert_eq!(
        doctor::zsh_block("~/h.zsh"),
        "if [[ -r ~/h.zsh ]]; then\n  source ~/h.zsh\nfi"
    );
}
