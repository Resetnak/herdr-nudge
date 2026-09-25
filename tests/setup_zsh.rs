//! `setup-zsh` against a scratch `HOME` and `ZDOTDIR`, never the real
//! `.zshrc`.

mod support;

use std::fs;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use herdr_nudge::setup_zsh::{self, Outcome};
use herdr_nudge::shell_hook::zsh_block;
use support::scratch_dir;

const HOOK_FROM_HOME: &str = ".local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh";

struct Scratch {
    home: PathBuf,
    zdotdir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = scratch_dir(name);
        let (home, zdotdir) = (dir.join("home"), dir.join("zdotdir"));
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&zdotdir).unwrap();
        Scratch { home, zdotdir }
    }

    fn zshrc(&self) -> PathBuf {
        self.zdotdir.join(".zshrc")
    }

    fn read_zshrc(&self) -> Option<String> {
        fs::read_to_string(self.zshrc()).ok()
    }

    /// The real binary, with only `HOME` and `ZDOTDIR`, answering `input`.
    fn run(&self, input: &str) -> (Output, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_herdr-nudge"))
            .arg("setup-zsh")
            .env_clear()
            .env("HOME", &self.home)
            .env("ZDOTDIR", &self.zdotdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        (out, stdout)
    }
}

fn added(block_path: &str) -> String {
    format!(
        "# Herdr Nudge: a notification when a long command finishes\n{}\n",
        zsh_block(block_path)
    )
}

#[test]
fn appends_to_the_zdotdir_zshrc_once_and_zsh_loads_the_hook_from_it() {
    let s = Scratch::new("setup_zsh_appends_once");
    fs::write(s.zshrc(), "export A=1").unwrap();
    let hook = s.home.join(HOOK_FROM_HOME);
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, "hook_loaded=yes\n").unwrap();

    let (out, stdout) = s.run("y\n");
    assert!(out.status.success(), "first run: {stdout}");
    let expected = format!("export A=1\n\n{}", added(&format!("~/{HOOK_FROM_HOME}")));
    assert_eq!(s.read_zshrc().as_deref(), Some(expected.as_str()));
    assert!(
        !s.home.join(".zshrc").exists(),
        "wrote ~/.zshrc though ZDOTDIR was set"
    );

    let (out, stdout) = s.run("y\n");
    assert!(out.status.success(), "second run: {stdout}");
    assert!(
        !stdout.contains("[y/N]"),
        "second run asked again: {stdout}"
    );
    assert_eq!(
        s.read_zshrc().as_deref(),
        Some(expected.as_str()),
        "second run changed .zshrc"
    );

    let zsh = Command::new("/bin/zsh")
        .args([
            "-f",
            "-c",
            r#"source "$ZDOTDIR/.zshrc"; print -r -- "$A $hook_loaded""#,
        ])
        .env_clear()
        .env("HOME", &s.home)
        .env("ZDOTDIR", &s.zdotdir)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&zsh.stdout), "1 yes\n");
}

#[test]
fn declining_changes_nothing() {
    let s = Scratch::new("setup_zsh_declining");
    for input in ["n\n", "", "yep\n"] {
        let (out, stdout) = s.run(input);
        assert!(out.status.success(), "answer {input:?}: {stdout}");
        assert_eq!(s.read_zshrc(), None, "answer {input:?} created .zshrc");
    }

    fs::write(s.zshrc(), "export A=1\n").unwrap();
    for input in ["n\n", ""] {
        s.run(input);
        assert_eq!(
            s.read_zshrc().as_deref(),
            Some("export A=1\n"),
            "answer {input:?} changed .zshrc"
        );
    }
}

fn run_in(rc: &Path, home: &Path, answer: &str) -> (Outcome, String) {
    let hook = home.join(HOOK_FROM_HOME);
    let mut out = Vec::new();
    let outcome = setup_zsh::run(rc, &hook, home, &mut Cursor::new(answer), &mut out).unwrap();
    (outcome, String::from_utf8(out).unwrap())
}

#[test]
fn the_lines_start_the_file_or_follow_a_blank_line() {
    let s = Scratch::new("setup_zsh_spacing");
    let block = added(&format!("~/{HOOK_FROM_HOME}"));

    assert_eq!(run_in(&s.zshrc(), &s.home, "y\n").0, Outcome::Appended);
    assert_eq!(s.read_zshrc(), Some(block.clone()), "new .zshrc");

    fs::write(s.zshrc(), "export A=1\n").unwrap();
    assert_eq!(run_in(&s.zshrc(), &s.home, "Y\n").0, Outcome::Appended);
    assert_eq!(s.read_zshrc(), Some(format!("export A=1\n\n{block}")));
}

/// A commented-out hook stays off, so that's a failure; another spelling
/// may be the same file, so it isn't.
#[test]
fn a_hook_commented_out_or_written_another_way_is_left_alone() {
    let s = Scratch::new("setup_zsh_left_alone");
    for (rc, exit_ok) in [
        (format!("# source ~/{HOOK_FROM_HOME}\n"), false),
        (
            "source \"$XDG_STATE_HOME/herdr/plugins/herdr-nudge/herdr-nudge.zsh\"\n".to_owned(),
            true,
        ),
    ] {
        fs::write(s.zshrc(), &rc).unwrap();
        let (out, said) = s.run("y\n");
        assert_eq!(out.status.success(), exit_ok, "exit for {rc:?}: {said}");
        assert!(!said.contains("[y/N]"), "asked for {rc:?}: {said}");
        assert_eq!(s.read_zshrc(), Some(rc));
    }
}

/// Stands in for a user who edits `.zshrc` while the question is up: the
/// edit happens when the answer is read.
struct EditThenAnswer<F: FnMut()> {
    edit: Option<F>,
    answer: Cursor<&'static [u8]>,
}

impl<F: FnMut()> Read for EditThenAnswer<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(mut edit) = self.edit.take() {
            edit();
        }
        self.answer.read(buf)
    }
}

fn run_editing(s: &Scratch, edit: impl FnMut()) -> Outcome {
    let hook = s.home.join(HOOK_FROM_HOME);
    let mut input = BufReader::new(EditThenAnswer {
        edit: Some(edit),
        answer: Cursor::new(b"y\n"),
    });
    setup_zsh::run(&s.zshrc(), &hook, &s.home, &mut input, &mut io::sink()).unwrap()
}

#[test]
fn zshrc_is_read_again_after_the_answer() {
    let s = Scratch::new("setup_zsh_read_again");
    let block = added(&format!("~/{HOOK_FROM_HOME}"));

    fs::write(s.zshrc(), "export A=1\n").unwrap();
    let outcome = run_editing(&s, || {
        fs::write(s.zshrc(), "export A=1\nexport B=2").unwrap()
    });
    assert_eq!(outcome, Outcome::Appended);
    assert_eq!(
        s.read_zshrc(),
        Some(format!("export A=1\nexport B=2\n\n{block}")),
        "the newline went by the text from before the question"
    );

    fs::write(s.zshrc(), "export A=1\n").unwrap();
    let outcome = run_editing(&s, || {
        fs::write(s.zshrc(), format!("export A=1\n\n{block}")).unwrap()
    });
    assert_eq!(
        outcome,
        Outcome::AlreadyLoads,
        "another run added the lines"
    );
    assert_eq!(s.read_zshrc(), Some(format!("export A=1\n\n{block}")));
}

#[test]
fn a_zshrc_that_isnt_utf8_still_gets_the_lines() {
    let s = Scratch::new("setup_zsh_not_utf8");
    fs::write(s.zshrc(), b"# caf\xe9\n").unwrap();
    assert_eq!(run_in(&s.zshrc(), &s.home, "y\n").0, Outcome::Appended);
    let mut expected = b"# caf\xe9\n\n".to_vec();
    expected.extend(added(&format!("~/{HOOK_FROM_HOME}")).as_bytes());
    assert_eq!(fs::read(s.zshrc()).unwrap(), expected);
}
