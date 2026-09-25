//! `herdr-nudge setup-zsh`: adds the lines that load the zsh hook to
//! `.zshrc`, after showing them and asking.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::shell_hook::{Zshrc, read_zshrc, shown_path, zsh_block, zshrc_loads};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Appended,
    AlreadyLoads,
    Declined,
    /// Left alone: someone commented it out, so it stays their call.
    CommentedOut,
    /// Left alone: `.zshrc` loads a `herdr-nudge.zsh` written some other
    /// way, like `$XDG_STATE_HOME/…`, which may well be the same file.
    OtherPath,
}

/// Errors reading or writing `.zshrc` carry its path. Errors on `input` or
/// `out` don't, since they're not about the file.
pub fn run(
    zshrc: &Path,
    hook: &Path,
    home: &Path,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<Outcome> {
    let about_rc = |e: io::Error| io::Error::new(e.kind(), format!("{}: {e}", zshrc.display()));
    let rc_name = shown_path(zshrc, home);
    let shown = shown_path(hook, home);
    let block = zsh_block(&shown);

    let rc = read_zshrc(zshrc).map_err(about_rc)?;
    if let Some(done) = settled(&rc, hook, home, &rc_name, &shown, &block, out)? {
        return Ok(done);
    }

    writeln!(
        out,
        "This adds these lines to the end of {rc_name}:\n\n{block}\n"
    )?;
    // Right after `herdr plugin install` this is the usual case: Herdr only
    // writes the hook when its server starts.
    if !hook.is_file() {
        writeln!(
            out,
            "{shown} isn't there yet. Herdr writes it when it starts, so restart Herdr too.\n"
        )?;
    }
    write!(out, "Add them? [y/N] ")?;
    out.flush()?;
    let mut answer = Vec::new();
    input.read_until(b'\n', &mut answer)?;
    let answer = String::from_utf8_lossy(&answer).trim().to_ascii_lowercase();
    if answer != "y" && answer != "yes" {
        writeln!(out, "Left {rc_name} unchanged.")?;
        return Ok(Outcome::Declined);
    }

    // Again, because the question can wait as long as the user likes: an
    // editor may have saved the file meanwhile, or another run added the
    // lines.
    let rc = read_zshrc(zshrc).map_err(about_rc)?;
    if let Some(done) = settled(&rc, hook, home, &rc_name, &shown, &block, out)? {
        return Ok(done);
    }
    let gap = if rc.is_empty() {
        ""
    } else if rc.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    // Appending follows a symlink, which is what a dotfiles setup wants:
    // the file zsh reads is the one behind it.
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(zshrc)
        .and_then(|mut file| {
            file.write_all(
                format!(
                    "{gap}# Herdr Nudge: a notification when a long command finishes\n{block}\n"
                )
                .as_bytes(),
            )
        })
        .map_err(about_rc)?;
    writeln!(out, "Added to {rc_name}. Open a new shell to load it.")?;
    Ok(Outcome::Appended)
}

/// What to say, and stop on, when `.zshrc` already mentions the hook.
fn settled(
    rc: &str,
    hook: &Path,
    home: &Path,
    rc_name: &str,
    shown: &str,
    block: &str,
    out: &mut impl Write,
) -> io::Result<Option<Outcome>> {
    let outcome = match zshrc_loads(rc, hook, home) {
        Zshrc::Missing => return Ok(None),
        Zshrc::Loads => {
            writeln!(out, "{rc_name} already loads the zsh hook. Nothing to do.")?;
            Outcome::AlreadyLoads
        }
        Zshrc::CommentedOut => {
            writeln!(
                out,
                "{rc_name} has the zsh hook, but commented out. Left it alone."
            )?;
            Outcome::CommentedOut
        }
        Zshrc::OtherPath => {
            writeln!(
                out,
                "{rc_name} already loads a herdr-nudge.zsh, written differently from {shown}.\n\
                 If it's the same file, there's nothing to do. If not, replace those lines with:\n\n{block}"
            )?;
            Outcome::OtherPath
        }
    };
    Ok(Some(outcome))
}
