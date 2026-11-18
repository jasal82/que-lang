//! Wall-clock limits and cooperative cancellation.
//!
//! Nothing here is process-global: a token belongs to whoever created it, and
//! an interpreter only sees the token and deadline its embedder gave it. That
//! is what lets several interpreters run on different threads and be
//! cancelled independently (the CLI's SIGINT flag in `crate::interrupt` is a
//! separate, process-wide mechanism that embedded interpreters ignore).

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A flag that asks running scripts to stop.
///
/// Cloning shares the flag: cancel any clone and every interpreter holding
/// one stops at its next check (statement boundary, loop iteration, or while
/// waiting in `sleep` or on a child process). `defer` blocks still run, and
/// a script can't catch the cancellation with `try`.
#[derive(Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Why a script stopped before finishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The embedder's [`CancellationToken`] was cancelled.
    Cancelled,
    /// The wall-clock limit (the value given to the engine) ran out.
    TimedOut(Duration),
}

impl fmt::Display for CancelReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CancelReason::Cancelled => write!(f, "script was cancelled"),
            CancelReason::TimedOut(limit) => {
                write!(f, "script exceeded its time limit of {:?}", limit)
            }
        }
    }
}

/// `Signal::Interrupted` numbers for the two reasons. Negative, so they
/// can't be mistaken for a real signal.
pub(crate) const SIGNAL_CANCELLED: i32 = -1;
pub(crate) const SIGNAL_TIMED_OUT: i32 = -2;

impl CancelReason {
    /// The unwind that carries this reason through the interpreter.
    pub(crate) fn into_signal(self) -> crate::error::Signal {
        crate::error::Signal::Interrupted(match self {
            CancelReason::Cancelled => SIGNAL_CANCELLED,
            CancelReason::TimedOut(_) => SIGNAL_TIMED_OUT,
        })
    }
}

/// The limits one interpreter runs under. Copied into every interpreter the
/// script causes to exist (imported modules, `parallel` branches), so they
/// all stop together.
#[derive(Clone, Default, Debug)]
pub(crate) struct Limits {
    pub cancel: Option<CancellationToken>,
    pub deadline: Option<(Instant, Duration)>,
}

impl Limits {
    pub fn is_active(&self) -> bool {
        self.cancel.is_some() || self.deadline.is_some()
    }

    /// The reason to stop now, if there is one.
    pub fn exceeded(&self) -> Option<CancelReason> {
        if self
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Some(CancelReason::Cancelled);
        }
        match self.deadline {
            Some((at, limit)) if Instant::now() >= at => Some(CancelReason::TimedOut(limit)),
            _ => None,
        }
    }

    /// Sleep for `total`, waking up regularly to check the limits.
    pub fn sleep(&self, total: Duration) -> Result<(), CancelReason> {
        if !self.is_active() {
            std::thread::sleep(total);
            return Ok(());
        }
        let end = Instant::now() + total;
        loop {
            if let Some(reason) = self.exceeded() {
                return Err(reason);
            }
            let now = Instant::now();
            if now >= end {
                return Ok(());
            }
            std::thread::sleep((end - now).min(POLL_INTERVAL));
        }
    }
}

/// How often blocking waits look at the limits. Short enough that a cancel
/// feels immediate, long enough not to burn CPU.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(20);
