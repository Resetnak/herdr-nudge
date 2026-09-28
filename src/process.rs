//! Running other programs.
//!
//! Everything that runs `herdr` or `lsappinfo` goes through [`Runner`], so
//! tests can replay captured output and check the argv without running
//! anything.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// `None` if the program was killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

pub trait Runner {
    /// Runs `program` with `args` directly, never through a shell.
    fn run(&self, program: &Path, args: &[&str]) -> io::Result<Output>;

    /// [`Runner::run`] with its own time limit, for the one call that can
    /// be slow on a busy Mac. A replay has no clock, so by default the
    /// limit is ignored.
    fn run_with_timeout(
        &self,
        program: &Path,
        args: &[&str],
        _timeout: Duration,
    ) -> io::Result<Output> {
        self.run(program, args)
    }
}

/// Starting a program and not waiting for it.
///
/// Separate from [`Runner`] so that posting a notification has no way to
/// wait. Herdr doesn't wait for a hook, but a hook that stayed open for the
/// notifier would hold its own banner back.
pub trait Spawner {
    fn spawn(&self, program: &Path, args: &[String]) -> io::Result<()>;
}

/// Runs real processes, and gives up on any that take longer than
/// `timeout`, whether it's the program that won't exit or its output that
/// won't arrive.
///
/// Herdr doesn't wait for hooks, so a hung `herdr` call holds up only its
/// own event. It would still leave a process behind until the timeout, and
/// that event's banner with it.
pub struct System {
    pub timeout: Duration,
}

impl Default for System {
    fn default() -> Self {
        System {
            timeout: Duration::from_secs(2),
        }
    }
}

impl Runner for System {
    fn run(&self, program: &Path, args: &[&str]) -> io::Result<Output> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Read both pipes on their own threads. Waiting first and reading
        // after would deadlock once the output fills the pipe buffer, and
        // `pane list` output grows with every pane.
        let stdout = child.stdout.take().map(read_all);
        let stderr = child.stderr.take().map(read_all);

        let deadline = Instant::now() + self.timeout;
        let timed_out = || {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{} took longer than {:?}", program.display(), self.timeout),
            )
        };

        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(timed_out());
            }
            thread::sleep(Duration::from_millis(5));
        };

        // The pipes can outlive the child: anything it started that
        // inherited them holds them open, and then the reads wait for that
        // process instead. So they get the same deadline.
        Ok(Output {
            code: status.code(),
            stdout: collect(stdout, deadline).ok_or_else(timed_out)?,
            stderr: collect(stderr, deadline).ok_or_else(timed_out)?,
        })
    }

    fn run_with_timeout(
        &self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> io::Result<Output> {
        System { timeout }.run(program, args)
    }
}

impl Spawner for System {
    /// The child is left running with its pipes closed. It outlives us: we
    /// exit within milliseconds and launchd takes over as its parent.
    fn spawn(&self, program: &Path, args: &[String]) -> io::Result<()> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

fn read_all(mut pipe: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// `None` if the output hadn't arrived by `deadline`. The reading thread is
/// left behind; the process it's waiting on is about to exit anyway.
fn collect(pipe: Option<Receiver<Vec<u8>>>, deadline: Instant) -> Option<String> {
    let Some(pipe) = pipe else {
        return Some(String::new());
    };
    let left = deadline.saturating_duration_since(Instant::now());
    let bytes = pipe.recv_timeout(left).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
