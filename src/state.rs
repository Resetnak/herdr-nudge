//! Files under `$HERDR_PLUGIN_STATE_DIR`.
//!
//! Every write goes to a new temp file in the same directory and is renamed
//! over the target, so a crash or a concurrent reader never sees half a
//! file. A file we can't parse is renamed aside and treated as missing: the
//! state here is all rebuildable, and a hook that fails on it every time
//! would stop notifications for good.
//!
//! Nothing is locked. The click process can run while an event hook does,
//! so two writers to the same file means the last one wins.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Bumped when a file's shape changes. A file with any other version is
/// moved aside like a corrupt one.
pub const VERSION: u32 = 1;

/// How long after our own click we refuse to learn a terminal. The click
/// brings the terminal forward, but for a moment Notification Center or the
/// previous app can still be frontmost.
pub const FOCUS_ORIGIN_TTL_MS: u64 = 15_000;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Writes `bytes` to `path` so that readers see the old file or the new
/// one, never a mix.
///
/// If `path` is a symlink, the rename replaces the link itself; the file it
/// pointed at is left alone.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    let dir = dir.unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;

    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let tmp = dir.join(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        next_tmp_counter()
    ));

    let result = (|| {
        // create_new fails if anything, a symlink included, is already there.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn next_tmp_counter() -> u32 {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

#[derive(Debug)]
pub enum Loaded<T> {
    Missing,
    Found(T),
    /// The file couldn't be used and has been moved out of the way.
    Recovered(Recovered),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub path: PathBuf,
    /// `None` if it couldn't be renamed and was deleted instead.
    pub moved_to: Option<PathBuf>,
    pub reason: String,
}

impl<T: Default> Loaded<T> {
    pub fn into_value(self) -> T {
        match self {
            Loaded::Found(value) => value,
            Loaded::Missing | Loaded::Recovered(_) => T::default(),
        }
    }
}

/// Reads a state file.
///
/// A symlink is moved aside, not followed. The click runs on behalf of our
/// notifier app, and a read that wandered into `~/Documents` would put up a
/// permission prompt under our name.
pub fn read_json<T: DeserializeOwned + Versioned>(path: &Path) -> io::Result<Loaded<T>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
        Err(e) => return Err(e),
    };
    if !meta.is_file() {
        return set_aside(path, "not a regular file".to_owned()).map(Loaded::Recovered);
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        // Another of our processes moved it aside between the two calls.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
        Err(e) => return Err(e),
    };
    let reason = match serde_json::from_slice::<T>(&bytes) {
        Ok(value) if value.version() == VERSION => return Ok(Loaded::Found(value)),
        Ok(value) => format!("version {}, expected {VERSION}", value.version()),
        Err(e) => e.to_string(),
    };
    set_aside(path, reason).map(Loaded::Recovered)
}

fn set_aside(path: &Path, reason: String) -> io::Result<Recovered> {
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".corrupt-{}", now_ms()));
    let aside = PathBuf::from(aside);

    let moved_to = match fs::rename(path, &aside) {
        Ok(()) => Some(aside),
        Err(_) => match fs::remove_file(path) {
            Ok(()) => None,
            // Someone else already dealt with it, which is all we wanted.
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        },
    };
    Ok(Recovered {
        path: path.to_owned(),
        moved_to,
        reason,
    })
}

/// State files carry a version so a newer build's files aren't misread.
pub trait Versioned {
    fn version(&self) -> u32;
}

/// `terminal-memory.json`: terminals we learned for workspaces that have
/// nothing in the config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalMemory {
    pub version: u32,
    #[serde(default)]
    pub workspaces: BTreeMap<String, LearnedTerminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnedTerminal {
    pub bundle_id: String,
    pub learned_at_ms: u64,
}

impl Default for TerminalMemory {
    fn default() -> Self {
        TerminalMemory {
            version: VERSION,
            workspaces: BTreeMap::new(),
        }
    }
}

impl Versioned for TerminalMemory {
    fn version(&self) -> u32 {
        self.version
    }
}

impl TerminalMemory {
    pub fn get(&self, workspace_id: &str) -> Option<&str> {
        self.workspaces
            .get(workspace_id)
            .map(|l| l.bundle_id.as_str())
    }

    pub fn set(&mut self, workspace_id: &str, bundle_id: &str, now_ms: u64) {
        self.workspaces.insert(
            workspace_id.to_owned(),
            LearnedTerminal {
                bundle_id: bundle_id.to_owned(),
                learned_at_ms: now_ms,
            },
        );
    }
}

/// `focus-origin.json`: when we last focused a pane in each workspace
/// ourselves, from a notification click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusOrigin {
    pub version: u32,
    #[serde(default)]
    pub workspaces: BTreeMap<String, u64>,
}

impl Default for FocusOrigin {
    fn default() -> Self {
        FocusOrigin {
            version: VERSION,
            workspaces: BTreeMap::new(),
        }
    }
}

impl Versioned for FocusOrigin {
    fn version(&self) -> u32 {
        self.version
    }
}

impl FocusOrigin {
    /// Also drops expired entries, so the file stays small.
    pub fn mark(&mut self, workspace_id: &str, now_ms: u64) {
        self.workspaces
            .retain(|_, at| now_ms < at.saturating_add(FOCUS_ORIGIN_TTL_MS));
        self.workspaces.insert(workspace_id.to_owned(), now_ms);
    }

    /// A mark from the future (the clock went back) still counts. Skipping
    /// one learn is cheap; learning the wrong terminal isn't.
    pub fn is_recent(&self, workspace_id: &str, now_ms: u64) -> bool {
        self.workspaces
            .get(workspace_id)
            .is_some_and(|at| now_ms < at.saturating_add(FOCUS_ORIGIN_TTL_MS))
    }
}

/// The state directory and the files in it.
#[derive(Debug, Clone)]
pub struct StateDir {
    pub root: PathBuf,
}

impl StateDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        StateDir { root: root.into() }
    }

    pub fn terminal_memory_path(&self) -> PathBuf {
        self.root.join("terminal-memory.json")
    }

    pub fn focus_origin_path(&self) -> PathBuf {
        self.root.join("focus-origin.json")
    }

    pub fn shell_env_path(&self) -> PathBuf {
        self.root.join("shell.env")
    }

    pub fn terminal_memory(&self) -> io::Result<Loaded<TerminalMemory>> {
        read_json(&self.terminal_memory_path())
    }

    pub fn save_terminal_memory(&self, memory: &TerminalMemory) -> io::Result<()> {
        write_json(&self.terminal_memory_path(), memory)
    }

    pub fn focus_origin(&self) -> io::Result<Loaded<FocusOrigin>> {
        read_json(&self.focus_origin_path())
    }

    pub fn save_focus_origin(&self, origin: &FocusOrigin) -> io::Result<()> {
        write_json(&self.focus_origin_path(), origin)
    }
}
