//! The zsh hook, installed the way `--cleanup` installs it and run in a real
//! interactive zsh (`zsh -f -i`, so none of the machine's rc files load). A
//! stub `herdr` writes down every call instead of talking to a server.
//!
//! zsh runs preexec and precmd for commands read from a pipe too, so no pty
//! is needed. Lines written all at once are what typing ahead looks like to
//! the hook; a `Pause` between them is a person waiting at the prompt. The
//! threshold is 1 s and the long commands sleep for 1.6 s, so each test
//! takes a few seconds.

mod support;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use herdr_nudge::shell_hook;
use herdr_nudge::state::{AgentsCache, StateDir};
use support::{Replay, scratch_dir};

const PANE: &str = "w9:p1";
const SOURCE: &str = "herdr-nudge-zsh";

/// Logs each call as one line, arguments split by the unit separator so a
/// title with spaces stays one argument.
const STUB: &str = "#!/bin/sh\nIFS=$(printf '\\037')\nprintf '%s\\n' \"$*\" >> \"$STUB_LOG\"\n";

enum Step<'a> {
    Line(&'a str),
    Pause(u64),
    /// SIGKILL the shell, so no precmd or zshexit runs.
    Kill,
}

use Step::{Kill, Line, Pause};

struct Shell {
    dir: PathBuf,
    state: StateDir,
    log: PathBuf,
    /// Extra environment for zsh, or a variable to leave out.
    env: Vec<(&'static str, Option<String>)>,
}

impl Shell {
    /// A state directory with the hook and `shell.env` written by the real
    /// installer, from `config` (a `config.toml`) and an agent list with
    /// `claude` in it.
    fn new(test: &str, config: &str) -> Shell {
        let dir = scratch_dir(test);
        let config_dir = dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.toml"), config).unwrap();

        let state = StateDir::new(dir.join("state"));
        state
            .save_agents_cache(&AgentsCache::new(["claude".to_owned()], 0))
            .unwrap();
        let notes = shell_hook::install(&state, Some(&config_dir), None, None, &Replay::new([]));
        assert!(
            notes.iter().any(|n| n.starts_with("zsh hook written")),
            "install notes: {notes:?}"
        );

        let stub = dir.join("herdr");
        fs::write(&stub, STUB).unwrap();
        make_executable(&stub);
        // The first run of a new executable can take seconds while macOS
        // checks it. Get that over with here, or a watcher's call could land
        // after a test has stopped looking for it.
        let warm = Command::new(&stub)
            .env("STUB_LOG", "/dev/null")
            .status()
            .unwrap();
        assert!(warm.success());

        Shell {
            log: dir.join("herdr-calls.log"),
            dir,
            state,
            env: Vec::new(),
        }
    }

    fn env(mut self, name: &'static str, value: Option<&str>) -> Shell {
        self.env.push((name, value.map(str::to_owned)));
        self
    }

    /// Sources the hook and writes `lines` all at once, so each is typed
    /// ahead of the one before. See [`Shell::steps`].
    fn run(&self, lines: &[&str]) -> String {
        let steps: Vec<Step> = lines.iter().map(|l| Line(l)).collect();
        self.steps(&steps)
    }

    /// Sources the hook, then feeds zsh `steps`, then closes its input, which
    /// exits the shell without an `exit` command. Returns once zsh has
    /// exited.
    fn steps(&self, steps: &[Step]) -> String {
        let stderr_path = self.dir.join("zsh-stderr.log");

        let mut cmd = Command::new("/bin/zsh");
        cmd.args(["-f", "-i"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.dir)
            .env("HERDR_ENV", "1")
            .env("HERDR_PANE_ID", PANE)
            .env("HERDR_BIN_PATH", self.dir.join("herdr"))
            .env("STUB_LOG", &self.log)
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            // Files, not pipes: a background job left running would hold a
            // pipe open and the test would wait for it.
            .stdout(fs::File::create(self.dir.join("zsh-stdout.log")).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap());
        for (name, value) in &self.env {
            match value {
                Some(v) => cmd.env(name, v),
                None => cmd.env_remove(name),
            };
        }

        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        // Write errors are ignored: after a Kill, or an exec, nobody reads.
        let _ = writeln!(stdin, "source {}", self.state.shell_hook_path().display());
        for step in steps {
            match step {
                Line(line) => {
                    let _ = writeln!(stdin, "{line}");
                }
                Pause(ms) => thread::sleep(Duration::from_millis(*ms)),
                Kill => {
                    child.kill().unwrap();
                    break;
                }
            }
        }
        drop(stdin);

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("zsh still running after 20 s");
            }
            thread::sleep(Duration::from_millis(20));
        }

        let stderr = fs::read_to_string(&stderr_path).unwrap();
        // zsh names the file or the function in any error it prints.
        assert!(
            !stderr.contains("herdr-nudge.zsh:") && !stderr.contains("_herdr_nudge_"),
            "zsh reported an error from the hook:\n{stderr}"
        );
        stderr
    }

    /// Every call so far, once `count` have arrived (or after 5 s), plus
    /// anything that turns up in the next half second.
    fn calls(&self, count: usize) -> Vec<Vec<String>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.read_calls().len() < count && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(500));
        self.read_calls()
    }

    /// No call at all, waiting long enough for a watcher that wasn't killed
    /// to have fired.
    fn assert_no_calls(&self) {
        thread::sleep(Duration::from_millis(1500));
        let calls = self.read_calls();
        assert!(calls.is_empty(), "expected no herdr calls, got {calls:#?}");
    }

    fn read_calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|l| l.split('\u{1f}').map(str::to_owned).collect())
            .collect()
    }
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

fn arg_after<'a>(call: &'a [String], flag: &str) -> Option<&'a str> {
    let i = call.iter().position(|a| a == flag)?;
    call.get(i + 1).map(String::as_str)
}

fn seq(call: &[String]) -> u64 {
    arg_after(call, "--seq")
        .unwrap_or_else(|| panic!("no --seq in {call:?}"))
        .parse()
        .unwrap()
}

/// The one call whose first three words are `pane <verb> <pane>` and that
/// has `extra` (a flag and its value) in it.
fn find<'a>(calls: &'a [Vec<String>], verb: &str, extra: (&str, &str)) -> &'a [String] {
    let found: Vec<_> = calls
        .iter()
        .filter(|c| c.len() > 2 && c[0] == "pane" && c[1] == verb && c[2] == PANE)
        .filter(|c| arg_after(c, extra.0) == Some(extra.1))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "pane {verb} with {} {}: {calls:#?}",
        extra.0,
        extra.1
    );
    found[0]
}

const CONFIG: &str = "[shell]\nmin_seconds = 1\nignore_commands = [\"vim\"]\n";

#[test]
fn a_command_under_the_threshold_never_calls_herdr() {
    let shell = Shell::new("zsh_short", CONFIG);
    shell.run(&["sleep 0.3", "true", "false"]);
    shell.assert_no_calls();
}

#[test]
fn a_long_command_reports_working_then_its_result_then_releases() {
    let shell = Shell::new("zsh_long", CONFIG);
    let before = now_us();
    shell.run(&["sleep 1.6", "true"]);
    let calls = shell.calls(5);
    assert_eq!(calls.len(), 5, "{calls:#?}");
    for call in &calls {
        assert_eq!(arg_after(call, "--source"), Some(SOURCE), "{call:?}");
        assert!(seq(call) > before, "--seq below the clock: {call:?}");
    }

    // The watcher replaces the last command's title before it claims the
    // pane.
    let clear = find(&calls, "report-metadata", ("--title", "sleep 1.6"));
    assert!(
        clear.contains(&"--clear-state-labels".to_owned()),
        "{clear:?}"
    );
    let working = find(&calls, "report-agent", ("--state", "working"));
    assert_eq!(arg_after(working, "--agent"), Some("sleep"));

    let result = find(&calls, "report-metadata", ("--state-label", "idle=done"));
    assert_eq!(
        arg_after(result, "--title"),
        Some("sleep 1.6 · exit 0 · 1s")
    );
    let idle = find(&calls, "report-agent", ("--state", "idle"));
    assert_eq!(arg_after(idle, "--agent"), Some("sleep"));

    // `true` was typed ahead, so it leaves the claim alone; the shell
    // exiting releases it.
    let release = find(&calls, "release-agent", ("--agent", "sleep"));

    // Herdr keeps one --seq for reports and releases, and another for
    // metadata, and drops anything that doesn't go up.
    assert!(seq(working) < seq(idle) && seq(idle) < seq(release));
    assert!(seq(clear) < seq(result));
}

fn releases(calls: &[Vec<String>]) -> Vec<&Vec<String>> {
    calls.iter().filter(|c| c[1] == "release-agent").collect()
}

#[test]
fn a_command_typed_at_the_prompt_releases_the_claim() {
    // The pause outlasts `sleep 1.6` by more than a second, so `true` starts
    // well after the prompt came back.
    let shell = Shell::new("zsh_release_at_prompt", CONFIG);
    shell.steps(&[
        Line("sleep 1.6"),
        Pause(3000),
        Line("true"),
        Line("sleep 1.6"),
    ]);
    let calls = shell.calls(10);
    let released = releases(&calls);
    assert_eq!(
        released.len(),
        2,
        "one from `true`, one on exit: {calls:#?}"
    );
    let first_idle = calls
        .iter()
        .filter(|c| arg_after(c, "--state") == Some("idle"))
        .map(|c| seq(c))
        .min()
        .unwrap();
    let first_release = released.iter().map(|c| seq(c)).min().unwrap();
    assert!(first_idle < first_release, "release before the first idle");
}

#[test]
fn a_command_typed_ahead_keeps_the_claim() {
    // `true` starts milliseconds after `sleep 1.6` ends. Releasing then would
    // take down, or race, the banner for a command the user walked away from.
    let shell = Shell::new("zsh_typed_ahead", CONFIG);
    shell.run(&["sleep 1.6", "true", "sleep 1.6"]);
    let calls = shell.calls(9);
    let released = releases(&calls);
    assert_eq!(
        released.len(),
        1,
        "only the shell exiting releases: {calls:#?}"
    );
    let last_idle = calls
        .iter()
        .filter(|c| arg_after(c, "--state") == Some("idle"))
        .map(|c| seq(c))
        .max()
        .unwrap();
    assert!(seq(released[0]) > last_idle);
}

#[test]
fn a_failed_long_command_is_labelled_failed() {
    let shell = Shell::new("zsh_failed", CONFIG);
    shell.run(&["sleep 1.6; false"]);
    let calls = shell.calls(5);
    let result = find(&calls, "report-metadata", ("--state-label", "idle=failed"));
    assert_eq!(
        arg_after(result, "--title"),
        Some("sleep 1.6; false · exit 1 · 1s")
    );
}

#[test]
fn the_shell_exiting_releases_the_claim() {
    // No command after the long one: end of input exits the shell with no
    // preexec, so only zshexit can release.
    let shell = Shell::new("zsh_exit", CONFIG);
    shell.run(&["sleep 1.6"]);
    let calls = shell.calls(5);
    let idle = find(&calls, "report-agent", ("--state", "idle"));
    let release = find(&calls, "release-agent", ("--agent", "sleep"));
    assert!(seq(idle) < seq(release));
}

#[test]
fn ignored_commands_and_agent_clis_are_not_reported() {
    // Functions stand in for the real programs. `nap` is an alias, so only
    // its expansion is on the lists. Quotes and an assignment with a path in
    // it must not hide the name.
    let shell = Shell::new("zsh_skipped", CONFIG);
    shell.run(&[
        "vim() { sleep 1.3 }",
        "claude() { sleep 1.3 }",
        "alias nap=vim",
        "vim",
        "claude",
        "nap",
        "\\vim",
        "'vim' a",
        "EDITOR=/opt/x vim",
        "PATH=/opt/bin:$PATH claude",
    ]);
    shell.assert_no_calls();
}

#[test]
fn exec_is_not_timed() {
    // The shell is replaced, so nothing would ever end the claim. Neither
    // form starts with `exec` as typed.
    for (name, lines) in [
        ("zsh_exec_chain", &["cd . && exec sleep 1.3"][..]),
        (
            "zsh_exec_alias",
            &["alias again='exec sleep 1.3'", "again"][..],
        ),
    ] {
        let shell = Shell::new(name, CONFIG);
        shell.run(lines);
        shell.assert_no_calls();
    }
}

#[test]
fn a_killed_shell_leaves_no_claim() {
    // SIGKILL runs no zshexit. The watcher outlives the shell and must not
    // report once its deadline comes.
    let shell = Shell::new("zsh_killed", CONFIG);
    shell.steps(&[Line("sleep 3"), Pause(300), Kill]);
    shell.assert_no_calls();
}

#[test]
fn sourcing_again_after_shell_commands_are_turned_off_removes_the_hooks() {
    // What `source ~/.zshrc` in an open pane does after `[shell] enabled =
    // false` and a Herdr restart.
    let shell = Shell::new("zsh_resourced_off", CONFIG);
    let env = shell.state.shell_env_path();
    let hook = shell.state.shell_hook_path();
    let off = format!("print -r -- enabled=0 > {}", env.display());
    let again = format!("source {}", hook.display());
    shell.run(&[&off, &again, "sleep 1.3"]);
    shell.assert_no_calls();
}

#[test]
fn the_hook_does_nothing_outside_a_herdr_pane() {
    let shell = Shell::new("zsh_outside", CONFIG).env("HERDR_ENV", None);
    shell.run(&["sleep 1.3"]);
    shell.assert_no_calls();
}

#[test]
fn the_hook_does_nothing_when_shell_commands_are_disabled() {
    let shell = Shell::new(
        "zsh_disabled",
        "[shell]\nenabled = false\nmin_seconds = 1\n",
    );
    shell.run(&["sleep 1.3"]);
    shell.assert_no_calls();
}

#[test]
fn the_hook_does_nothing_without_shell_env() {
    let shell = Shell::new("zsh_no_env", CONFIG);
    fs::remove_file(shell.state.shell_env_path()).unwrap();
    shell.run(&["sleep 1.3"]);
    shell.assert_no_calls();
}

#[test]
fn the_hook_stands_aside_for_herdr_ohmyzsh() {
    let shell = Shell::new("zsh_omz", CONFIG);
    shell.run(&["_herdr_omz_preexec() { }", "sleep 1.3"]);
    shell.assert_no_calls();
}
