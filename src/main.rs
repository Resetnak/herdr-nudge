//! Dispatch. Event mode parses its input and logs it; the rest is still to
//! come.

use std::process::ExitCode;

use herdr_nudge::cli::{self, Mode, ParseError};
use herdr_nudge::context::{self, Context, Env};
use herdr_nudge::{event, event_summary};

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        Ok(Mode::Event) => run_event(),
        Ok(Mode::Help) => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Mode::Version) => {
            println!("herdr-nudge {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(other) => {
            eprintln!("herdr-nudge: {} is not implemented yet", mode_name(&other));
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("herdr-nudge: {err}");
            eprint!("{}", cli::USAGE);
            match err {
                // A bad --click means a stale or stray notification, not a
                // bug: say so and stop.
                ParseError::BadJobId(_) => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Logs to stderr and exits 0 whatever happens. A payload we can't parse
/// isn't our bug, and exiting non-zero would just look like a broken plugin.
fn run_event() -> ExitCode {
    let ctx = match std::env::var(context::CONTEXT_JSON_VAR) {
        Ok(json) => match Context::parse(&json) {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                eprintln!(
                    "herdr-nudge: unparseable {}: {err}",
                    context::CONTEXT_JSON_VAR
                );
                None
            }
        },
        Err(_) => None,
    };

    match Env::from_process() {
        Ok(env) => eprintln!(
            "herdr-nudge: state_dir={} config_dir={}",
            env.state_dir.display(),
            env.config_dir.display()
        ),
        Err(missing) => eprintln!("herdr-nudge: incomplete environment ({missing})"),
    }

    let Ok(json) = std::env::var(context::EVENT_JSON_VAR) else {
        eprintln!("herdr-nudge: no {}, nothing to do", context::EVENT_JSON_VAR);
        return ExitCode::SUCCESS;
    };

    match event::Envelope::parse(&json) {
        Ok(envelope) => eprintln!("herdr-nudge: {}", event_summary(&envelope, ctx.as_ref())),
        Err(err) => eprintln!(
            "herdr-nudge: unparseable {}: {err}",
            context::EVENT_JSON_VAR
        ),
    }

    ExitCode::SUCCESS
}

fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Event => "event mode",
        Mode::Click(_) => "--click",
        Mode::Cleanup => "--cleanup",
        Mode::Doctor => "doctor",
        Mode::Bind(_) => "bind",
        Mode::Test { .. } => "test",
        Mode::Help => "--help",
        Mode::Version => "--version",
    }
}
