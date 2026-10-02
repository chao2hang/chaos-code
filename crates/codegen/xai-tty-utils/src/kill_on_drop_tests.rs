use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::KillOnDrop;

/// A child that outlives the test unless the guard ends it: `sleep` on Unix,
/// `ping`'s own repeat counter on Windows. Both are the direct child, so the
/// guard's `kill()` reaches the process whose PID the assertions follow.
fn spawn_sleeper() -> std::process::Child {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("ping");
        c.args(["-n", "300", "127.0.0.1"]);
        c
    } else {
        let mut c = Command::new("sleep");
        c.arg("300");
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::detach_std_command(&mut cmd);
    #[allow(clippy::disallowed_methods)] // test fixture; guarded/reaped by the test
    cmd.spawn().expect("spawn sleeper")
}

/// Zombie-tolerant bounded probe: the contract is that the child stops
/// *running*; whether the corpse is reaped promptly is environmental.
fn assert_stops_running(pid: u32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !crate::process_not_running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        crate::process_not_running(pid),
        "{what} still running after drop"
    );
}

#[test]
fn drop_kills_and_reaps_the_child() {
    let guard = KillOnDrop::new(spawn_sleeper());
    let pid = guard.id();
    assert!(!crate::process_not_running(pid), "sanity: sleeper running");

    drop(guard);

    assert_stops_running(pid, "KillOnDrop child");
}

#[test]
fn into_inner_disarms_without_killing() {
    let guard = KillOnDrop::new(spawn_sleeper());
    let pid = guard.id();

    let mut child = guard.into_inner();

    assert!(
        !crate::process_not_running(pid),
        "into_inner must release the child without killing it"
    );
    child.kill().expect("kill released child");
    child.wait().expect("reap released child");
}

#[test]
fn drop_after_in_handle_reap_is_a_no_op() {
    // Something that exits 0 at once. `cmd` runs its own `exit` builtin, so
    // unlike `cmd /C <program>` there is no grandchild behind the guarded PID.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "exit", "0"]);
        c
    } else {
        Command::new("true")
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::detach_std_command(&mut cmd);
    #[allow(clippy::disallowed_methods)] // test fixture; reaped through the guard
    let mut guard = KillOnDrop::new(cmd.spawn().expect("spawn the exit-0 fixture"));

    let status = guard.wait().expect("in-handle reap through the guard");
    assert!(status.success(), "the fixture exits 0");

    // Drop after the in-handle reap must not panic and must not signal a
    // recycled PID (std's Child::kill refuses already-waited children).
    drop(guard);
}
