//! What `--cleanup` writes for the zsh hook. `tests/zsh_hook.rs` runs the
//! result in a real zsh.

mod support;

use std::fs;
use std::path::Path;

use herdr_nudge::config::{Config, shell_env};
use herdr_nudge::shell_hook::{self, SCRIPT};
use herdr_nudge::state::{AgentsCache, StateDir};
use support::{Recorded, Replay, scratch_dir};

const HERDR: &str = "/Users/dev/.local/bin/herdr";

fn state_with_agents(dir: &Path) -> StateDir {
    let state = StateDir::new(dir.join("state"));
    state
        .save_agents_cache(&AgentsCache::new(
            ["claude".to_owned(), "codex".to_owned()],
            0,
        ))
        .unwrap();
    state
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn install_writes_the_hook_and_settings_from_the_config() {
    let dir = scratch_dir("install_from_config");
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let toml = "[shell]\nmin_seconds = 45\nignore_commands = [\"vim\"]\n";
    fs::write(config_dir.join("config.toml"), toml).unwrap();
    let state = state_with_agents(&dir);
    let replay = Replay::new([]);

    let notes = shell_hook::install(
        &state,
        Some(&config_dir),
        Some(Path::new(HERDR)),
        None,
        &replay,
    );

    assert_eq!(read(&state.shell_hook_path()), SCRIPT);
    let config = Config::parse(toml).unwrap();
    let agents = config.agent_catalogue(["claude", "codex"]);
    assert_eq!(read(&state.shell_env_path()), shell_env(&config, &agents).0);
    assert_eq!(
        replay.call_count(),
        0,
        "herdr asked though the config dir was given"
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
}

#[test]
fn install_asks_herdr_for_the_config_dir_when_the_env_lacks_it() {
    let dir = scratch_dir("install_asks_herdr");
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        "[shell]\nmin_seconds = 90\n",
    )
    .unwrap();
    let state = state_with_agents(&dir);
    // The captured answer, pointed at this test's config.
    let mut recorded = Recorded::cli("plugin-config-dir");
    recorded.stdout = format!("{}\n", config_dir.display());
    let replay = Replay::new([recorded]);

    let notes = shell_hook::install(&state, None, Some(Path::new(HERDR)), None, &replay);

    assert!(read(&state.shell_env_path()).contains("\nmin_seconds=90\n"));
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("no HERDR_PLUGIN_CONFIG_DIR")),
        "{notes:?}"
    );
    assert_eq!(
        replay.calls.borrow()[0][1..],
        ["plugin", "config-dir", "herdr-nudge"]
    );
}

#[test]
fn install_writes_the_defaults_when_the_config_cant_be_read() {
    let dir = scratch_dir("install_defaults");
    let state = state_with_agents(&dir);
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), "[shell\n").unwrap();

    for (config_dir, why) in [
        (None, "no config directory"),
        (Some(config_dir.as_path()), "defaults"),
    ] {
        let notes = shell_hook::install(&state, config_dir, None, None, &Replay::new([]));
        let env = read(&state.shell_env_path());
        assert!(env.contains("\nenabled=1\nmin_seconds=5\n"), "{why}: {env}");
        assert!(notes.iter().any(|n| n.contains(why)), "{why}: {notes:?}");
        assert_eq!(read(&state.shell_hook_path()), SCRIPT, "{why}");
    }
}

#[test]
fn install_without_an_agent_list_says_so() {
    let dir = scratch_dir("install_no_agents");
    let state = StateDir::new(dir.join("state"));

    let notes = shell_hook::install(&state, None, None, None, &Replay::new([]));

    // Only the agents Herdr integrates without a manifest are left.
    let env = read(&state.shell_env_path());
    let agents: Vec<_> = env.lines().filter(|l| l.starts_with("agent=")).collect();
    assert_eq!(agents, ["agent=mastracode", "agent=omp"], "{env}");
    assert!(
        notes.iter().any(|n| n.contains("no agent list")),
        "{notes:?}"
    );
}

#[test]
fn install_prefers_the_list_just_fetched_to_the_saved_one() {
    // Saving the fresh list can fail; the hook should still get it.
    let dir = scratch_dir("install_fetched_list");
    let state = state_with_agents(&dir);
    let fetched = AgentsCache::new(["letta".to_owned()], 0);

    shell_hook::install(&state, None, None, Some(fetched), &Replay::new([]));

    let env = read(&state.shell_env_path());
    assert!(env.contains("agent=letta\n"), "{env}");
    assert!(!env.contains("agent=claude\n"), "saved list used: {env}");
}
