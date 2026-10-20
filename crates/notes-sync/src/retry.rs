//! Retry policy: exponential backoff, plus whatever wait the server asked for.

use std::cell::RefCell;

use async_trait::async_trait;

/// Attempts one sync run makes before it gives up and reports the error.
pub const MAX_ATTEMPTS: u32 = 5;

/// First wait after a failed attempt, in seconds.
pub const RETRY_BASE_SECS: i64 = 1;

/// Longest wait the exponential part of the backoff reaches.
pub const RETRY_MAX_SECS: i64 = 60;

/// Longest wait the engine will honour from a `retry-after` value.
///
/// The server's login backoff caps at 300 seconds, and waiting less than it
/// asks would only produce another `429`.
pub const SERVER_WAIT_MAX_SECS: i64 = 300;

/// Seconds to wait after failure number `attempt` (1 for the first failure).
///
/// The wait grows 1, 2, 4, 8, … up to [`RETRY_MAX_SECS`]. A server-supplied
/// `retry-after` wins whenever it is longer, so a rate-limited client waits as
/// long as the server needs.
pub fn backoff_seconds(attempt: u32, retry_after: Option<i64>) -> i64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let exponential = RETRY_BASE_SECS
        .saturating_mul(1i64 << exponent)
        .min(RETRY_MAX_SECS);
    match retry_after {
        Some(seconds) => seconds.clamp(0, SERVER_WAIT_MAX_SECS).max(exponential),
        None => exponential,
    }
}

/// Waiting, so the engine can be tested without real delays.
#[async_trait(?Send)]
pub trait Sleeper {
    /// Wait `seconds` before the next attempt.
    async fn sleep(&self, seconds: i64);
}

/// Waits on the current thread.
///
/// Blocking, like the native transport: use it on a sync worker thread, not on
/// a UI thread.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub struct ThreadSleeper;

#[cfg(not(target_arch = "wasm32"))]
#[async_trait(?Send)]
impl Sleeper for ThreadSleeper {
    async fn sleep(&self, seconds: i64) {
        std::thread::sleep(std::time::Duration::from_secs(seconds.max(0) as u64));
    }
}

/// Waits nothing and remembers what it was asked to wait.
///
/// Tests use the record; an app that schedules its own retry uses the type
/// itself and reads the error's `retry_after`.
#[derive(Debug, Default)]
pub struct NoSleep {
    waits: RefCell<Vec<i64>>,
}

impl NoSleep {
    /// A sleeper that never sleeps.
    pub fn new() -> Self {
        Self::default()
    }

    /// The waits it was asked for, in order.
    pub fn waits(&self) -> Vec<i64> {
        self.waits.borrow().clone()
    }
}

#[async_trait(?Send)]
impl Sleeper for NoSleep {
    async fn sleep(&self, seconds: i64) {
        self.waits.borrow_mut().push(seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::{backoff_seconds, NoSleep, Sleeper, RETRY_MAX_SECS, SERVER_WAIT_MAX_SECS};

    #[test]
    fn the_backoff_doubles_then_caps() {
        assert_eq!(backoff_seconds(1, None), 1);
        assert_eq!(backoff_seconds(2, None), 2);
        assert_eq!(backoff_seconds(3, None), 4);
        assert_eq!(backoff_seconds(4, None), 8);
        assert_eq!(backoff_seconds(20, None), RETRY_MAX_SECS);
    }

    #[test]
    fn a_server_wait_wins_when_it_is_longer() {
        assert_eq!(backoff_seconds(1, Some(5)), 5);
        assert_eq!(backoff_seconds(3, Some(2)), 4, "the exponential part wins");
        assert_eq!(backoff_seconds(7, Some(2)), RETRY_MAX_SECS);
        assert_eq!(backoff_seconds(1, Some(9_999)), SERVER_WAIT_MAX_SECS);
        assert_eq!(backoff_seconds(1, Some(-4)), 1);
    }

    #[test]
    fn the_recording_sleeper_keeps_the_order() {
        let sleeper = NoSleep::new();
        pollster::block_on(async {
            sleeper.sleep(3).await;
            sleeper.sleep(7).await;
        });
        assert_eq!(sleeper.waits(), vec![3, 7]);
    }
}
