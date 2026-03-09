//! Exponential-backoff retry, mirroring the Go client's `connection.Retry`.

use crate::errors::{ErrCode, GatewayDError};
use std::time::Duration;

/// Hard caps (when `disable_backoff_caps` is false).
pub const BACKOFF_MULTIPLIER_CAP: f64 = 10.0;
pub const BACKOFF_DURATION_CAP: Duration = Duration::from_secs(60);

/// Retry configuration and executor.
#[derive(Debug, Clone)]
pub struct Retry {
    /// Maximum number of attempts.  0 means unlimited.
    pub retries: usize,
    /// Initial backoff duration.
    pub backoff: Duration,
    /// Multiplier applied after each failed attempt.
    pub backoff_multiplier: f64,
    /// When true, caps are not applied.
    pub disable_backoff_caps: bool,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            retries: 3,
            backoff: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            disable_backoff_caps: false,
        }
    }
}

impl Retry {
    pub fn new(
        retries: usize,
        backoff: Duration,
        backoff_multiplier: f64,
        disable_backoff_caps: bool,
    ) -> Self {
        Self {
            retries,
            backoff,
            backoff_multiplier,
            disable_backoff_caps,
        }
    }

    /// Execute an async callback, retrying on error with exponential backoff.
    ///
    /// Returns `Ok(T)` on first success or `Err(GatewayDError)` after
    /// all retries are exhausted.
    pub async fn run<F, Fut, T>(&self, mut f: F) -> Result<T, GatewayDError>
    where
        F: FnMut(usize) -> Fut,
        Fut: std::future::Future<Output = Result<T, GatewayDError>>,
    {
        let max = self.retries;
        let mut delay = self.backoff.as_secs_f64();

        for attempt in 0.. {
            match f(attempt).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let last = max > 0 && attempt + 1 >= max;
                    if last {
                        return Err(GatewayDError::new(
                            ErrCode::RetriesExhausted,
                            format!(
                                "retries exhausted after {} attempts: {}",
                                attempt + 1,
                                e.message
                            ),
                        ));
                    }
                    let sleep_secs = apply_caps(delay, self.disable_backoff_caps);
                    tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;
                    delay *= self.backoff_multiplier;
                    if !self.disable_backoff_caps {
                        delay = delay.min(BACKOFF_DURATION_CAP.as_secs_f64());
                    }
                }
            }
        }
        unreachable!()
    }
}

fn apply_caps(secs: f64, disabled: bool) -> f64 {
    if disabled {
        secs
    } else {
        secs.min(BACKOFF_DURATION_CAP.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exhausted_does_not_panic() {
        let r = Retry {
            retries: 2,
            backoff: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            disable_backoff_caps: true,
        };
        let res: Result<(), _> = r
            .run(|_| async { Err(GatewayDError::new(ErrCode::Unknown, "always fail")) })
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let r = Retry::default();
        let res = r.run(|_| async { Ok::<i32, GatewayDError>(42) }).await;
        assert_eq!(res.unwrap(), 42);
    }

    #[tokio::test]
    async fn succeeds_after_failures() {
        let r = Retry {
            retries: 5,
            backoff: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            disable_backoff_caps: true,
        };
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = count.clone();
        let res = r
            .run(|_| {
                let c = c2.clone();
                async move {
                    let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if n < 3 {
                        Err(GatewayDError::new(ErrCode::Unknown, "fail"))
                    } else {
                        Ok(n)
                    }
                }
            })
            .await;
        assert!(res.is_ok());
    }
}
