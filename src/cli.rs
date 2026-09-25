//! Argument parsing. Every mode is either positional or a bare flag, so a
//! parser crate isn't worth the dependency. Pure function over argv: it
//! reads nothing else and never exits.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// No arguments: Herdr invoked us as an event hook.
    Event,
    /// macOS relaunched the bundle because a notification was clicked, and
    /// is running that notification's own command.
    Click(JobId),
    /// Herdr's startup hook.
    Cleanup,
    Doctor,
    /// Writes a commented `config.toml`, or prints it if there is one.
    ExampleConfig,
    SetupZsh,
    Test {
        shell: bool,
    },
    Help,
    Version,
}

/// A job id: 16 lowercase hex characters.
///
/// Checking in the constructor means an unchecked id can't exist. The id
/// comes back from macOS on a click and is used to name a file, and it's the
/// only thing we ever put into the notifier's `-execute` string, so it has
/// to be safe before anything uses it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(String);

impl JobId {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let ok = s.len() == 16 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if ok {
            Ok(JobId(s.to_owned()))
        } else {
            Err(ParseError::BadJobId(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnknownArg(String),
    BadJobId(String),
    MissingValue(&'static str),
    UnexpectedArg { mode: &'static str, arg: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnknownArg(a) => write!(f, "unknown argument: {a}"),
            ParseError::BadJobId(a) => {
                write!(f, "--click wants 16 lowercase hex characters, got: {a}")
            }
            ParseError::MissingValue(what) => write!(f, "missing value for {what}"),
            ParseError::UnexpectedArg { mode, arg } => {
                write!(f, "unexpected argument for {mode}: {arg}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

pub const USAGE: &str = "\
herdr-nudge — macOS notifications for Herdr panes

  herdr-nudge                       event hook (invoked by Herdr)
  herdr-nudge --click <16 hex>      focus the pane a notification was for
  herdr-nudge --cleanup             startup hook
  herdr-nudge doctor                diagnose config and bundle
  herdr-nudge test [--shell]        post a notification for this pane
  herdr-nudge example-config        write config.toml with every setting,
                                    or print it if the file exists
  herdr-nudge setup-zsh             add the zsh hook to .zshrc, after asking
";

/// `args` is argv without the program name.
pub fn parse<I, S>(args: I) -> Result<Mode, ParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut rest = args.iter().map(String::as_str);

    let Some(first) = rest.next() else {
        return Ok(Mode::Event);
    };

    match first {
        "--click" => {
            let value = rest.next().ok_or(ParseError::MissingValue("--click"))?;
            let id = JobId::parse(value)?;
            no_more(rest, "--click")?;
            Ok(Mode::Click(id))
        }
        "--cleanup" => {
            no_more(rest, "--cleanup")?;
            Ok(Mode::Cleanup)
        }
        "doctor" => {
            no_more(rest, "doctor")?;
            Ok(Mode::Doctor)
        }
        "example-config" => {
            no_more(rest, "example-config")?;
            Ok(Mode::ExampleConfig)
        }
        "setup-zsh" => {
            no_more(rest, "setup-zsh")?;
            Ok(Mode::SetupZsh)
        }
        "test" => {
            let mut shell = false;
            for arg in rest {
                match arg {
                    "--shell" => shell = true,
                    other => {
                        return Err(ParseError::UnexpectedArg {
                            mode: "test",
                            arg: other.to_owned(),
                        });
                    }
                }
            }
            Ok(Mode::Test { shell })
        }
        "--help" | "-h" | "help" => Ok(Mode::Help),
        "--version" | "-V" => Ok(Mode::Version),
        other => Err(ParseError::UnknownArg(other.to_owned())),
    }
}

fn no_more<'a>(
    mut rest: impl Iterator<Item = &'a str>,
    mode: &'static str,
) -> Result<(), ParseError> {
    match rest.next() {
        None => Ok(()),
        Some(arg) => Err(ParseError::UnexpectedArg {
            mode,
            arg: arg.to_owned(),
        }),
    }
}
