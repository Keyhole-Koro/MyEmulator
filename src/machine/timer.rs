use std::time::{Duration, Instant};

// Real-time timer. Fires (returns true from `service`) once at least `period`
// of wall-clock time has elapsed since the last fire, independent of how many
// instructions the CPU executed. This lets the guest use WFI/halt to idle the
// CPU without stalling the timer: real time keeps advancing and the next tick
// still arrives to wake the scheduler.
pub struct TimerDevice {
    // Microseconds between ticks. None disables the timer.
    period: Option<Duration>,
    last_fire: Instant,
}

impl TimerDevice {
    pub fn new() -> Self {
        Self {
            period: Some(Duration::from_micros(1000)),
            last_fire: Instant::now(),
        }
    }

    // Set the tick period from a microsecond count (the CLI's --timer-interval).
    pub fn set_period_micros(&mut self, micros: u64) {
        if micros == 0 {
            self.period = None;
        } else {
            self.period = Some(Duration::from_micros(micros));
        }
        self.last_fire = Instant::now();
    }

    // The configured tick period, or None when the timer is disabled.
    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    // Fire if a full period has elapsed. Still delivers at most one tick per
    // call, but advances last_fire by exactly one period rather than resetting
    // it to `now`: late servicing (batch granularity, a long-running handler)
    // then yields catch-up ticks on subsequent calls instead of silently
    // stretching every period, so the average rate stays wall-clock accurate
    // and guest tick counts don't drift. If we fall hopelessly behind (host
    // suspend, debugger pause), resync to `now` so the guest doesn't receive a
    // burst of stale ticks.
    pub fn service(&mut self) -> bool {
        if let Some(period) = self.period {
            let now = Instant::now();
            if now.duration_since(self.last_fire) >= period {
                self.last_fire += period;
                if now.duration_since(self.last_fire) > period * 32 {
                    self.last_fire = now;
                }
                return true;
            }
        }
        false
    }

    // How long until the next tick is due, or None when the timer is disabled.
    // Used to sleep the host thread while the guest is idle (WFI) instead of
    // busy-spinning.
    pub fn time_until_next(&self) -> Option<Duration> {
        self.period.map(|period| {
            let elapsed = self.last_fire.elapsed();
            period.checked_sub(elapsed).unwrap_or(Duration::ZERO)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TimerDevice;
    use std::time::Duration;

    #[test]
    fn late_service_catches_up_instead_of_stretching_the_period() {
        let mut t = TimerDevice::new();
        t.set_period_micros(10_000); // 10 ms
        std::thread::sleep(Duration::from_millis(25));
        assert!(t.service(), "first tick after a late poll");
        assert!(
            t.service(),
            "the missed tick is delivered as an immediate catch-up tick"
        );
    }

    #[test]
    fn disabled_timer_never_fires() {
        let mut t = TimerDevice::new();
        t.set_period_micros(0);
        assert!(!t.service());
        assert!(t.time_until_next().is_none());
    }
}
