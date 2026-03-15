use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Token-bucket rate limiter with exponential backoff for API calls.
/// Enforces both RPM (requests per minute) and RPD (requests per day) limits.
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
    config: RateLimiterConfig,
    daily_count: Arc<AtomicU64>,
    daily_limit: u64,
    /// Minimum interval between requests to stay within RPM limit.
    min_interval: Duration,
}

struct RateLimiterState {
    last_request: Option<Instant>,
    current_delay: Duration,
    consecutive_429s: u32,
}

#[derive(Clone)]
pub struct RateLimiterConfig {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_retries: u32,
    pub backoff_factor: f64,
    pub jitter_fraction: f64,
    /// Requests per minute limit (used to compute min_interval).
    pub rpm: u32,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_retries: 8,
            backoff_factor: 2.0,
            jitter_fraction: 0.25,
            rpm: 10,
        }
    }
}

impl RateLimiter {
    pub fn new(config: RateLimiterConfig, daily_limit: u64) -> Self {
        // Compute minimum interval from RPM: 60s / rpm, with 10% safety margin
        let min_interval_secs = 60.0 / config.rpm as f64 * 1.1;
        let min_interval = Duration::from_secs_f64(min_interval_secs);

        // Use the larger of base_delay or min_interval as initial delay
        let effective_base = config.base_delay.max(min_interval);

        Self {
            state: Arc::new(Mutex::new(RateLimiterState {
                last_request: None,
                current_delay: effective_base,
                consecutive_429s: 0,
            })),
            config,
            daily_count: Arc::new(AtomicU64::new(0)),
            daily_limit,
            min_interval,
        }
    }

    /// Wait before making a request. Returns Ok(()) when ready, Err if budget exhausted.
    pub async fn acquire(&self) -> anyhow::Result<()> {
        let count = self.daily_count.load(Ordering::Relaxed);
        let budget_threshold = (self.daily_limit as f64 * 0.9) as u64;

        if count >= budget_threshold && count < self.daily_limit {
            tracing::warn!(
                "Daily budget at {}% ({}/{}), consider switching to lite model",
                (count * 100) / self.daily_limit,
                count,
                self.daily_limit
            );
        }

        if count >= self.daily_limit {
            anyhow::bail!(
                "Daily request budget exhausted ({}/{})",
                count,
                self.daily_limit
            );
        }

        let mut state = self.state.lock().await;

        if let Some(last) = state.last_request {
            let elapsed = last.elapsed();
            // Always enforce minimum interval even outside backoff
            let effective_delay = state.current_delay.max(self.min_interval);
            if elapsed < effective_delay {
                let wait = effective_delay - elapsed;
                drop(state);
                tracing::debug!("Rate limiter: waiting {:?} before next request", wait);
                tokio::time::sleep(wait).await;
                state = self.state.lock().await;
            }
        }

        state.last_request = Some(Instant::now());
        self.daily_count.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Report a successful request — resets backoff to base.
    pub async fn report_success(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_429s = 0;
        state.current_delay = self.config.base_delay.max(self.min_interval);
    }

    /// Report a 429 rate limit — increase backoff with exponential delay + jitter.
    pub async fn report_rate_limit(&self, retry_after: Option<Duration>) {
        let mut state = self.state.lock().await;
        state.consecutive_429s += 1;

        if let Some(retry_after) = retry_after {
            // Respect server's Retry-After header, add small jitter
            let jitter = 1.0 + rand::random::<f64>() * 0.1;
            state.current_delay = Duration::from_secs_f64(retry_after.as_secs_f64() * jitter);
        } else {
            // Exponential backoff: base * factor^consecutive_429s
            let delay_secs = self.config.base_delay.as_secs_f64()
                * self
                    .config
                    .backoff_factor
                    .powi(state.consecutive_429s as i32);

            // Add ±25% jitter to avoid thundering herd
            let jitter = 1.0 + (rand::random::<f64>() * 2.0 - 1.0) * self.config.jitter_fraction;
            let delay_with_jitter = delay_secs * jitter;

            state.current_delay =
                Duration::from_secs_f64(delay_with_jitter.min(self.config.max_delay.as_secs_f64()));
        }

        tracing::warn!(
            "Rate limited (429 #{}) — backing off {:?}",
            state.consecutive_429s,
            state.current_delay
        );
    }

    pub fn daily_count(&self) -> u64 {
        self.daily_count.load(Ordering::Relaxed)
    }

    pub fn max_retries(&self) -> u32 {
        self.config.max_retries
    }

    pub fn daily_limit(&self) -> u64 {
        self.daily_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_success() {
        let limiter = RateLimiter::new(RateLimiterConfig::default(), 250);
        limiter.acquire().await.unwrap();
        assert_eq!(limiter.daily_count(), 1);
    }

    #[tokio::test]
    async fn test_daily_budget_exhaustion() {
        let limiter = RateLimiter::new(
            RateLimiterConfig {
                base_delay: Duration::from_millis(1),
                rpm: 1000, // high RPM so we don't wait
                ..Default::default()
            },
            2,
        );
        limiter.acquire().await.unwrap();
        limiter.report_success().await;
        limiter.acquire().await.unwrap();
        limiter.report_success().await;
        // Third should fail
        let result = limiter.acquire().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_backoff_increases() {
        let limiter = RateLimiter::new(RateLimiterConfig::default(), 250);
        limiter.report_rate_limit(None).await;
        let state = limiter.state.lock().await;
        assert!(state.current_delay >= Duration::from_secs(1));
        assert!(state.consecutive_429s == 1);
    }

    #[tokio::test]
    async fn test_retry_after_respected() {
        let limiter = RateLimiter::new(RateLimiterConfig::default(), 250);
        limiter
            .report_rate_limit(Some(Duration::from_secs(30)))
            .await;
        let state = limiter.state.lock().await;
        // Should be at least 30s (with small jitter)
        assert!(state.current_delay >= Duration::from_secs(30));
        assert!(state.current_delay < Duration::from_secs(35));
    }

    #[tokio::test]
    async fn test_rpm_spacing() {
        // 10 RPM = 6s min interval, with 10% margin = 6.6s
        let config = RateLimiterConfig {
            rpm: 10,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config, 250);
        assert!(limiter.min_interval >= Duration::from_secs(6));
        assert!(limiter.min_interval < Duration::from_secs(8));
    }
}
