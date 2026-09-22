//! Running real programs: output capture and the timeout.

use std::path::Path;
use std::time::{Duration, Instant};

use herdr_nudge::process::{Runner, Spawner, System};

fn system(ms: u64) -> System {
    System {
        timeout: Duration::from_millis(ms),
    }
}

fn sh(script: &str) -> Vec<&str> {
    vec!["-c", script]
}

const SH: &str = "/bin/sh";

#[test]
fn captures_stdout_stderr_and_exit_code() {
    let out = system(2000)
        .run(Path::new(SH), &sh("echo out; echo err >&2; exit 3"))
        .unwrap();
    assert_eq!(out.stdout, "out\n");
    assert_eq!(out.stderr, "err\n");
    assert_eq!(out.code, Some(3));
    assert!(!out.success());
}

/// More output than a pipe buffer holds. Waiting for the child before
/// reading would deadlock here.
#[test]
fn captures_output_larger_than_a_pipe_buffer() {
    let out = system(5000)
        .run(
            Path::new(SH),
            &sh("for i in $(seq 1 20000); do echo line$i; done"),
        )
        .unwrap();
    assert_eq!(out.stdout.lines().count(), 20000);
    assert!(out.success());
}

#[test]
fn a_program_that_never_exits_is_killed() {
    let started = Instant::now();
    let err = system(200)
        .run(Path::new(SH), &sh("sleep 30"))
        .expect_err("should have timed out");

    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited {:?}",
        started.elapsed()
    );
}

/// The child exits at once but leaves a background process holding its
/// stdout, so reading the pipe waits on that process instead. Without a
/// deadline on the reads this hangs for as long as the grandchild lives.
#[test]
fn output_held_open_by_a_grandchild_times_out() {
    let started = Instant::now();
    let err = system(200)
        .run(Path::new(SH), &sh("sleep 30 & echo done"))
        .expect_err("should have timed out");

    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited {:?}",
        started.elapsed()
    );
}

#[test]
fn a_missing_program_is_an_error() {
    let err = system(200)
        .run(Path::new("/nonexistent/herdr"), &[])
        .expect_err("should have failed to spawn");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

/// Herdr runs an event hook synchronously and waits for it, so posting a
/// notification must not wait for the notifier. `Runner` waits; `Spawner`
/// must not.
#[test]
fn spawning_returns_before_the_child_does() {
    let system = System::default();
    let started = Instant::now();
    system
        .spawn(Path::new("/bin/sleep"), &["5".to_owned()])
        .expect("sleep should start");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "spawning /bin/sleep 5 took {elapsed:?}; it should return at once"
    );
}

/// The same command through `Runner` does wait, which is the difference the
/// two traits exist to make.
#[test]
fn running_waits_for_the_child() {
    let system = System {
        timeout: Duration::from_millis(300),
    };
    let started = Instant::now();
    let result = system.run(Path::new("/bin/sleep"), &["5"]);
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "sleep 5 should hit the timeout, got {result:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(250),
        "run returned after {elapsed:?}; it should have waited for the timeout"
    );
}
