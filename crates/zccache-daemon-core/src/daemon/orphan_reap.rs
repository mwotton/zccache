//! Reaping reparented orphan children of the daemon process (zombie hygiene).
//!
//! A long-lived daemon that runs as its container's PID 1 is the reaper of
//! last resort for every orphan in its PID namespace, whether or not it
//! spawned the process. The shared-CI deployment on honorable (ghr-rz7) met
//! exactly this: docker runs each container healthcheck probe as a shell
//! whose `zccache status --json` child outlives the shell whenever the probe
//! is killed at its timeout. The orphan finishes its status request, exits,
//! is reparented to the daemon — and stayed a `[zccache] <defunct>` forever,
//! because every wait path in this daemon is pid-targeted and nothing ever
//! reaps a child this process did not spawn. ~1,600 zombies accumulated in
//! 33 hours (one per timed-out probe); the namespace's PID supply is finite,
//! so an unbounded zombie pool eventually starves compiler spawns.
//!
//! The fix is a periodic `waitpid(-1, WNOHANG)` drain, gated so it can never
//! race a session's own child wait: every child this daemon spawns on behalf
//! of a request belongs to a client session, and its exit status belongs to
//! that session's pidfd/`waitid` wait. Draining while any session is active
//! could consume that status first and wedge the session. The caller in
//! [`super::server::maintenance_schedule`] therefore only drains when no
//! session exists, and the drain itself is bounded to children that have
//! already exited.
//!
//! Standalone-only by declaration: an embedded host shares this process with
//! its own supervisor, whose children are not ours to reap.

/// Drain exited children no one will wait for.
///
/// Reaps every already-exited child of this process — including reparented
/// orphans this daemon never spawned — and returns how many were reaped.
/// Returns zero when no child has exited, so a caller can treat a nonzero
/// result as "there were zombies".
///
/// The caller must ensure no other thread is concurrently waiting on one of
/// this process's children (see the module header for the session gate that
/// guarantees this).
#[cfg(unix)]
pub(crate) fn drain_orphaned_children() -> usize {
    let mut reaped = 0;
    loop {
        // SAFETY: waitpid with a null status pointer and WNOHANG never
        // dereferences the pointer; we only need the return value.
        let rc = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if rc > 0 {
            reaped += 1;
            continue;
        }
        // 0: children exist but none has exited. Negative: no waitable child
        // (ECHILD) or an error we cannot act on — either way the drain ends.
        break;
    }
    reaped
}

/// Non-Unix hosts are not PID-1-orphan collectors the way Linux containers
/// are; their process models reap orphans in the platform's own layer.
#[cfg(not(unix))]
pub(crate) fn drain_orphaned_children() -> usize {
    0
}

#[cfg(test)]
#[path = "orphan_reap_tests.rs"]
mod tests;
