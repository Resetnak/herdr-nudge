//! The only place that runs the bundled `terminal-notifier`.
//!
//! Arguments go as an argv array, so a title full of quotes or newlines is
//! just a string. The one exception is `-execute`, which macOS hands to
//! `/bin/sh -c` when the notification is clicked. That value is built by
//! [`click_command`] and can only ever be our own binary path plus a job id
//! we generated — no event data goes near it.
//!
//! Nothing here waits: the notifier is started and left alone, which is why
//! posting takes a [`Spawner`] and not a `Runner`. There is nothing to wait
//! for: a click comes back later as a new process running `--click`.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::cli::JobId;
use crate::process::{Runner, Spawner};

/// Where the bundle sits inside the plugin, and the binary inside the bundle.
const BUNDLE: &str = "vendor/HerdrNudge.app";
const BINARY: &str = "Contents/MacOS/terminal-notifier";
const DEFAULTS: &str = "/usr/bin/defaults";

pub fn bundle_path(plugin_root: &Path) -> PathBuf {
    plugin_root.join(BUNDLE)
}

pub fn binary_path(plugin_root: &Path) -> PathBuf {
    bundle_path(plugin_root).join(BINARY)
}

/// Whether macOS is in dark mode right now.
///
/// `defaults` prints `Dark` in dark mode and fails in light mode, because
/// the key is only there while dark (`tests/fixtures/sys/defaults-appearance-*`).
/// A banner keeps the logo it was posted with, so after a switch the older
/// ones in Notification Center have the file for the old appearance.
pub fn dark_mode<R: Runner>(runner: &R) -> bool {
    runner
        .run(Path::new(DEFAULTS), &["read", "-g", "AppleInterfaceStyle"])
        .is_ok_and(|out| out.success() && out.stdout.trim() == "Dark")
}

/// Notifications for the same pane share a group, so a newer one replaces the
/// older banner instead of stacking up, and `-remove` can withdraw it.
///
/// The pane id goes in as-is. It is unique already, and it is an argv element
/// rather than a file name, so there is nothing to encode.
pub fn group_for(pane_id: &str) -> String {
    format!("herdr-nudge-{pane_id}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post<'a> {
    pub title: &'a str,
    /// None leaves the line out, and the banner has two lines, not three.
    pub subtitle: Option<&'a str>,
    pub message: &'a str,
    pub group: &'a str,
    /// The agent's logo, shown on the right of the banner. The left icon is
    /// the bundle's own and can't be set per notification.
    pub content_image: Option<&'a Path>,
    pub sound: bool,
    /// From [`click_command`].
    pub execute: &'a str,
}

/// Why our own path can't go into a shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadBinaryPath {
    Quote(PathBuf),
    Relative(PathBuf),
    NotUtf8(PathBuf),
}

impl fmt::Display for BadBinaryPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BadBinaryPath::Quote(p) => {
                write!(f, "our own path contains a single quote: {}", p.display())
            }
            BadBinaryPath::Relative(p) => {
                write!(f, "our own path is not absolute: {}", p.display())
            }
            BadBinaryPath::NotUtf8(p) => {
                write!(f, "our own path is not utf-8: {}", p.display())
            }
        }
    }
}

impl std::error::Error for BadBinaryPath {}

/// The `-execute` value: `'<our binary>' --click <16 hex>`, and nothing else.
///
/// macOS runs this through a shell, so the path is single-quoted and a path
/// containing a single quote is refused rather than escaped — we would rather
/// not notify than build a command by quoting rules. It must be absolute
/// because the click runs from an unknown directory.
pub fn click_command(binary: &Path, job: &JobId) -> Result<String, BadBinaryPath> {
    let path = binary
        .to_str()
        .ok_or_else(|| BadBinaryPath::NotUtf8(binary.to_owned()))?;
    if !binary.is_absolute() {
        return Err(BadBinaryPath::Relative(binary.to_owned()));
    }
    if path.contains('\'') {
        return Err(BadBinaryPath::Quote(binary.to_owned()));
    }
    Ok(format!("'{path}' --click {job}"))
}

pub fn post_args(post: &Post) -> Vec<String> {
    let mut args = vec![
        "-title".to_owned(),
        post.title.to_owned(),
        "-message".to_owned(),
        post.message.to_owned(),
        "-group".to_owned(),
        post.group.to_owned(),
        "-execute".to_owned(),
        post.execute.to_owned(),
    ];
    if let Some(subtitle) = post.subtitle {
        args.push("-subtitle".to_owned());
        args.push(subtitle.to_owned());
    }
    if let Some(image) = post.content_image {
        args.push("-contentImage".to_owned());
        args.push(image.display().to_string());
    }
    if post.sound {
        args.push("-sound".to_owned());
        args.push("default".to_owned());
    }
    args
}

pub fn remove_args(group: &str) -> Vec<String> {
    vec!["-remove".to_owned(), group.to_owned()]
}

pub struct Notifier<'a, S: Spawner> {
    pub binary: &'a Path,
    pub spawner: &'a S,
}

impl<S: Spawner> Notifier<'_, S> {
    pub fn post(&self, post: &Post) -> std::io::Result<()> {
        self.spawner.spawn(self.binary, &post_args(post))
    }

    /// Withdraws a delivered notification, whether it is still on screen or
    /// sitting in Notification Center.
    pub fn remove(&self, group: &str) -> std::io::Result<()> {
        self.spawner.spawn(self.binary, &remove_args(group))
    }
}
