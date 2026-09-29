//! Regression tests for the orphan-child drain (ghr-rz7).
//!
//! These tests spawn real children and deliberately do not wait for them, so
//! they must not run alongside another test that reaps children of this
//! process. The assertions live in one sequential test, and the repository
//! suite runs serially.

#[cfg(target_os = "linux")]
use super::drain_orphaned_children;

/// The `/proc` state character for `pid`, when its entry still exists.
#[cfg(target_os = "linux")]
fn proc_state(pid: libc::pid_t) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The comm field is parenthesized and may contain spaces; the state is
    // the field after the closing parenthesis.
    let after_comm = stat.rsplit(')').next()?;
    after_comm.split_whitespace().next()?.chars().next()
}

#[cfg(target_os = "linux")]
fn wait_until_zombie(child: &mut std::process::Child) -> bool {
    let pid = child.id() as libc::pid_t;
    for _ in 0..100 {
        if proc_state(pid) == Some('Z') {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[cfg(target_os = "linux")]
fn proc_gone(pid: libc::pid_t) -> bool {
    !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// The drain reaps exited-but-unwaited children (the production shape: an
/// orphan reparented to the daemon) and never touches a live child.
#[cfg(target_os = "linux")]
#[test]
fn drain_reaps_orphans_without_disturbing_live_children() {
    // A child that is still running is untouched by a drain round.
    let live = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let live_pid = live.id() as libc::pid_t;
    assert_eq!(
        drain_orphaned_children(),
        0,
        "a drain with no exited child must reap nothing"
    );
    assert!(
        !proc_gone(live_pid) && proc_state(live_pid) != Some('Z'),
        "live child {live_pid} must not be touched by the drain"
    );

    // A child that exited without anyone waiting for it is reaped by the
    // drain and disappears from /proc entirely.
    let mut exiting = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let exiting_pid = exiting.id() as libc::pid_t;
    assert!(
        wait_until_zombie(&mut exiting),
        "child {exiting_pid} should have exited into the zombie state"
    );
    std::mem::forget(exiting);
    let reaped = drain_orphaned_children();
    assert!(
        reaped >= 1,
        "drain should reap at least the zombie child {exiting_pid}"
    );
    assert!(
        proc_gone(exiting_pid),
        "zombie child {exiting_pid} should be fully reaped, not left in /proc"
    );

    // Once the live child exits un-waited, the next drain reaps it too.
    let mut live = live;
    live.kill().expect("kill sleep child");
    assert!(
        wait_until_zombie(&mut live),
        "killed child {live_pid} should be a zombie before the drain"
    );
    std::mem::forget(live);
    let reaped = drain_orphaned_children();
    assert!(
        reaped >= 1,
        "drain should reap the killed child {live_pid} once it has exited"
    );
    assert!(
        proc_gone(live_pid),
        "child {live_pid} should be fully reaped"
    );
}
