use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

const WINDOW: Duration = Duration::from_secs(2);
const MIN_GAP: Duration = Duration::from_millis(50);

/// Sliding-window throughput meter; robust to being polled at any frequency.
#[derive(Debug, Default)]
pub struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    /// Records the running byte total and returns bytes per second over the window.
    pub fn record(&mut self, now: Instant, total: u64) -> u64 {
        if self
            .samples
            .back()
            .is_none_or(|&(at, _)| now.duration_since(at) >= MIN_GAP)
        {
            self.samples.push_back((now, total));
        }
        while self.samples.len() > 2
            && self
                .samples
                .front()
                .is_some_and(|&(at, _)| now.duration_since(at) > WINDOW)
        {
            self.samples.pop_front();
        }
        match (self.samples.front(), self.samples.back()) {
            (Some(&(t0, b0)), Some(&(t1, b1))) if t1 > t0 => {
                (b1.saturating_sub(b0) as f64 / t1.duration_since(t0).as_secs_f64()) as u64
            }
            _ => 0,
        }
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

/// Decides when a download should open one more connection: keep adding
/// while each addition still raises throughput by at least 5 %, then hold,
/// re-probing occasionally in case the network got faster.
#[derive(Debug, Default)]
pub struct RampController {
    prev_rate: f64,
    flat_ticks: u32,
}

impl RampController {
    const GAIN: f64 = 1.05;
    const REPROBE_AFTER: u32 = 8;

    pub fn should_add(&mut self, rate: f64) -> bool {
        let improved = rate > self.prev_rate * Self::GAIN;
        self.prev_rate = rate;
        if improved {
            self.flat_ticks = 0;
            return true;
        }
        self.flat_ticks += 1;
        if self.flat_ticks >= Self::REPROBE_AFTER {
            self.flat_ticks = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_window_rate() {
        let mut m = SpeedMeter::default();
        let t0 = Instant::now();
        assert_eq!(m.record(t0, 0), 0);
        assert_eq!(m.record(t0 + Duration::from_secs(1), 1_000), 1_000);
        assert_eq!(m.record(t0 + Duration::from_secs(2), 3_000), 1_500);
    }

    #[test]
    fn ramp_adds_while_throughput_grows() {
        let mut r = RampController::default();
        assert!(r.should_add(100.0));
        assert!(r.should_add(200.0));
        assert!(!r.should_add(201.0));
        for _ in 0..6 {
            assert!(!r.should_add(201.0));
        }
        assert!(r.should_add(201.0), "re-probes after a plateau");
    }
}
