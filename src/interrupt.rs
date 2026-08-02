//! Cooperative interrupt handling for SIGINT and SIGTERM.
//!
//! A signal handler cannot run `defer` blocks — it is not allowed to touch the
//! interpreter, allocate, or take a lock. So the handler does the only thing it
//! safely can: record which signal arrived. The interpreter polls that record
//! at statement boundaries and, when it sees one, unwinds with
//! [`Signal::Interrupted`](crate::error::Signal::Interrupted).
//!
//! Unwinding is what makes this worth doing: every `defer` on the stack runs on
//! the way out, so temp directories are removed, `spawn`ed dev servers are
//! killed, and containers are torn down instead of leaking on Ctrl-C.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// Signal number that arrived, or 0 for "none yet".
static PENDING: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
static INSTALLED: OnceLock<()> = OnceLock::new();

fn pending() -> &'static Arc<AtomicUsize> {
    PENDING.get_or_init(|| Arc::new(AtomicUsize::new(0)))
}

/// Install handlers for SIGINT and SIGTERM. Idempotent.
///
/// Only the CLI calls this. Embedders (and the test suite) run without
/// handlers, so they keep the host process's signal behaviour.
pub fn install() {
    INSTALLED.get_or_init(|| {
        #[cfg(unix)]
        {
            use signal_hook::consts::{SIGINT, SIGTERM};
            for sig in [SIGINT, SIGTERM] {
                let _ = signal_hook::flag::register_usize(
                    sig,
                    Arc::clone(pending()),
                    sig as usize,
                );
            }
        }
    });
}

/// The signal that arrived, if any.
pub fn pending_signal() -> Option<i32> {
    match pending().load(Ordering::Relaxed) {
        0 => None,
        n => Some(n as i32),
    }
}

/// Forget any pending signal. Used by the REPL, where Ctrl-C cancels the
/// current line rather than the session.
pub fn clear() {
    pending().store(0, Ordering::Relaxed);
}

/// The shell convention for a process killed by a signal: 128 + signal number.
/// SIGINT is 130, SIGTERM is 143.
pub fn exit_code_for(signal: i32) -> i32 {
    128 + signal
}

/// Human-readable name for the signal, for the message printed on exit.
pub fn name_for(signal: i32) -> &'static str {
    match signal {
        2 => "SIGINT",
        15 => "SIGTERM",
        _ => "signal",
    }
}
