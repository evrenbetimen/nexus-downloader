//! Global token-bucket bandwidth limiter shared by every connection.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::Instant;

#[derive(Debug)]
struct Bucket {
    /// May go negative: a connection that over-draws sleeps off the debt.
    tokens: f64,
    last_refill: Instant,
}

/// Token bucket whose rate can be changed while downloads are running.
#[derive(Debug)]
pub struct BandwidthLimiter {
    /// Bytes per second; 0 means unlimited.
    rate: AtomicU64,
    bucket: Mutex<Bucket>,
}

impl BandwidthLimiter {
    #[must_use]
    pub fn new(bytes_per_sec: Option<u64>) -> Self {
        Self {
            rate: AtomicU64::new(bytes_per_sec.map_or(0, |rate| rate.max(1))),
            bucket: Mutex::new(Bucket {
                tokens: 0.0,
                last_refill: Instant::now(),
            }),
        }
    }

    #[must_use]
    pub fn limit(&self) -> Option<u64> {
        match self.rate.load(Ordering::Relaxed) {
            0 => None,
            rate => Some(rate),
        }
    }

    /// Takes effect on the very next chunk of every running connection.
    pub fn set_limit(&self, bytes_per_sec: Option<u64>) {
        self.rate.store(
            bytes_per_sec.map_or(0, |rate| rate.max(1)),
            Ordering::Relaxed,
        );
        let mut bucket = self.bucket.lock().expect("limiter poisoned");
        bucket.tokens = 0.0;
        bucket.last_refill = Instant::now();
    }

    /// Waits until `bytes` may be consumed under the current limit.
    pub async fn acquire(&self, bytes: usize) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let rate = rate as f64;
        let wait = {
            let mut bucket = self.bucket.lock().expect("limiter poisoned");
            let now = Instant::now();
            let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.last_refill = now;
            // Allow a quarter second of burst so short stalls don't waste bandwidth.
            let burst = rate / 4.0;
            bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
            bucket.tokens -= bytes as f64;
            if bucket.tokens >= 0.0 {
                None
            } else {
                Some(Duration::from_secs_f64(-bucket.tokens / rate))
            }
        };
        if let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn throttles_to_configured_rate() {
        let limiter = BandwidthLimiter::new(Some(100_000));
        let start = Instant::now();
        for _ in 0..50 {
            limiter.acquire(10_000).await;
        }
        // 500 KB at 100 KB/s needs ~5 s (minus nothing: the bucket starts empty).
        let elapsed = start.elapsed().as_secs_f64();
        assert!((4.9..5.2).contains(&elapsed), "elapsed {elapsed}");
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_never_waits() {
        let limiter = BandwidthLimiter::new(None);
        let start = Instant::now();
        limiter.acquire(usize::MAX).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
        limiter.set_limit(Some(1));
        assert_eq!(limiter.limit(), Some(1));
        limiter.set_limit(None);
        assert_eq!(limiter.limit(), None);
    }
}
