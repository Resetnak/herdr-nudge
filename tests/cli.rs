//! Argument parsing.

use herdr_nudge::cli::{JobId, Mode, ParseError, parse};

#[test]
fn no_arguments_is_an_event_hook() {
    let empty: [&str; 0] = [];
    assert_eq!(parse(empty), Ok(Mode::Event));
}

#[test]
fn a_well_formed_job_id_is_accepted() {
    let Ok(Mode::Click(id)) = parse(["--click", "0123456789abcdef"]) else {
        panic!("expected a click");
    };
    assert_eq!(id.as_str(), "0123456789abcdef");
}

/// The id comes back from macOS on a click and is used to name a file, so
/// anything that isn't exactly 16 lowercase hex characters is refused.
#[test]
fn a_job_id_that_is_not_sixteen_lowercase_hex_is_refused() {
    let bad = [
        "",
        "0123456789abcde",     // 15
        "0123456789abcdef0",   // 17
        "0123456789ABCDEF",    // uppercase
        "0123456789abcdeg",    // not hex
        "../../../etc/passwd", // traversal
        "0123456789abcde/",    // separator
        "0123456789abcde.",    // extension games
        "0123456789abcd ef",   // space
        "0123456789abcdef'",   // quote
        "$(id)0123456789ab",   // substitution, right length
        "0123456789abcdef\n",  // trailing newline
    ];

    for value in bad {
        assert_eq!(
            JobId::parse(value),
            Err(ParseError::BadJobId(value.to_owned())),
            "{value:?} must not parse"
        );
        assert!(
            matches!(parse(["--click", value]), Err(ParseError::BadJobId(_))),
            "--click {value:?} must not parse"
        );
    }
}

#[test]
fn click_needs_its_value_and_nothing_more() {
    assert_eq!(parse(["--click"]), Err(ParseError::MissingValue("--click")));
    assert_eq!(
        parse(["--click", "0123456789abcdef", "extra"]),
        Err(ParseError::UnexpectedArg {
            mode: "--click",
            arg: "extra".into()
        })
    );
}

#[test]
fn the_remaining_modes_parse() {
    assert_eq!(parse(["--cleanup"]), Ok(Mode::Cleanup));
    assert_eq!(parse(["doctor"]), Ok(Mode::Doctor));
    assert_eq!(parse(["test"]), Ok(Mode::Test { shell: false }));
    assert_eq!(parse(["test", "--shell"]), Ok(Mode::Test { shell: true }));
    assert_eq!(parse(["--help"]), Ok(Mode::Help));
    assert_eq!(parse(["-h"]), Ok(Mode::Help));
    assert_eq!(parse(["--version"]), Ok(Mode::Version));
}

/// A typo shouldn't fall through to the event path.
#[test]
fn an_unknown_argument_is_refused_rather_than_ignored() {
    assert_eq!(
        parse(["--clean"]),
        Err(ParseError::UnknownArg("--clean".into()))
    );
    assert_eq!(parse(["bind"]), Err(ParseError::UnknownArg("bind".into())));
    assert_eq!(
        parse(["doctor", "--verbose"]),
        Err(ParseError::UnexpectedArg {
            mode: "doctor",
            arg: "--verbose".into()
        })
    );
    assert_eq!(
        parse(["test", "--shel"]),
        Err(ParseError::UnexpectedArg {
            mode: "test",
            arg: "--shel".into()
        })
    );
}
