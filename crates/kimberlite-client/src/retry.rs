//! Retry helper for Kimberlite operations.
//!
//! AUDIT-2026-04 S2.4 — ports notebar's retry idiom into the Rust
//! SDK so every app gets identical backoff semantics without
//! hand-rolling a wrapper.
//!
//! # Example
//!
//! ```no_run
//! use kimberlite_client::retry::{with_retry, RetryPolicy, DEFAULT_RETRY};
//! # use kimberlite_client::Client;
//! # fn dummy(_c: &mut Client) -> kimberlite_client::ClientResult<()> { Ok(()) }
//! # fn main() -> kimberlite_client::ClientResult<()> {
//! # let mut client: Client = todo!();
//! with_retry(DEFAULT_RETRY, || dummy(&mut client))?;
//! # Ok(())
//! # }
//! ```
//!
//! For transparent reconnect + retry on a single `ConnectionError`,
//! see [`crate::Client::invoke_with_reconnect`]. `with_retry` is
//! the broader retry primitive that applies backoff to any error
//! where `is_retryable()` returns true — typically
//! `RateLimited`, `NotLeader`, `ProjectionLag`, or transient I/O
//! failures.

use std::thread;
use std::time::Duration;

use crate::error::ClientResult;

/// Exponential-backoff retry policy.
///
/// - `max_attempts`: total attempts INCLUDING the initial call.
///   A value of 1 disables retries.
/// - `base_delay`: delay before the first retry.
/// - `cap_delay`: upper bound on the delay between attempts.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub cap_delay: Duration,
}

/// Sensible default: four attempts, 50 ms → 100 ms → 200 ms → 400 ms.
/// Total worst-case wall-clock overhead is ~750 ms before the
/// final error surfaces — fits the 2-second human-perception
/// budget for synchronous interactive calls.
pub const DEFAULT_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 4,
    base_delay: Duration::from_millis(50),
    cap_delay: Duration::from_millis(800),
};

/// Run `op` with exponential-backoff retries for errors whose
/// `is_retryable()` returns true. Non-retryable errors propagate
/// immediately.
///
/// The backoff doubles from `base_delay` up to `cap_delay`. Giving
/// up at `max_attempts` returns the most recent error.
pub fn with_retry<F, T>(policy: RetryPolicy, mut op: F) -> ClientResult<T>
where
    F: FnMut() -> ClientResult<T>,
{
    let mut attempt: u32 = 0;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                attempt = attempt.saturating_add(1);
                if attempt >= policy.max_attempts || !e.is_retryable() {
                    return Err(e);
                }
                let doubled = policy.base_delay.saturating_mul(1 << (attempt - 1));
                let wait = doubled.min(policy.cap_delay);
                thread::sleep(wait);
            }
        }
    }
}

// NOTE: An async variant of `with_retry` will land with S2.1
// (the native async Rust client). It cannot be added here yet
// because `kimberlite-client` does not currently depend on tokio
// — adding it now would pull an async runtime into every sync
// user.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientError;
    use kimberlite_wire::ErrorCode;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Fast policy so tests complete quickly.
    const FAST: RetryPolicy = RetryPolicy {
        max_attempts: 4,
        base_delay: Duration::from_millis(1),
        cap_delay: Duration::from_millis(4),
    };

    #[test]
    fn returns_op_result_on_first_success() {
        let calls = Cell::new(0);
        let result: ClientResult<i32> = with_retry(FAST, || {
            calls.set(calls.get() + 1);
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retries_retryable_error_and_eventually_succeeds() {
        let calls = AtomicU32::new(0);
        let result: ClientResult<&'static str> = with_retry(FAST, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(ClientError::server(ErrorCode::RateLimited, "slow down"))
            } else {
                Ok("ok")
            }
        });
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn non_retryable_error_is_not_retried() {
        let calls = AtomicU32::new(0);
        let result: ClientResult<i32> = with_retry(FAST, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(ClientError::server(ErrorCode::QueryParseError, "bad SQL"))
        });
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn gives_up_after_max_attempts() {
        let calls = AtomicU32::new(0);
        let result: ClientResult<i32> = with_retry(FAST, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(ClientError::server(ErrorCode::NotLeader, "elsewhere"))
        });
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), FAST.max_attempts);
    }

    #[test]
    fn max_attempts_1_disables_retry() {
        let calls = AtomicU32::new(0);
        let policy = RetryPolicy {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
            cap_delay: Duration::from_millis(1),
        };
        let result: ClientResult<i32> = with_retry(policy, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(ClientError::server(ErrorCode::RateLimited, "nope"))
        });
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_retry_uses_four_attempts() {
        // Sanity-check the public const matches documentation.
        assert_eq!(DEFAULT_RETRY.max_attempts, 4);
        assert_eq!(DEFAULT_RETRY.base_delay, Duration::from_millis(50));
        assert_eq!(DEFAULT_RETRY.cap_delay, Duration::from_millis(800));
    }
}
