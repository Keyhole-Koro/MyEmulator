use std::time::{Duration, Instant};

// Real-time timer. Fires (returns true from `service`) once at least `period`
// of wall-clock time has elapsed since the last fire, independent of how many
// instructions the CPU executed. This lets the guest use WFI/halt to idle the
// CPU without stalling the timer: real time keeps advancing and the next tick
// still arrives to wake the scheduler.
//
// A purely wall-clock timer has one failure mode: if the handler takes longer
// than a period, the next tick is already due the moment it returns, and the
// guest never runs anything else. How much guest work fits in a period depends
// on how fast this emulator happens to run, so the same image starves on one
// host and not another.
//
// MIN_INSTRS_PER_TICK is the floor that removes that. A tick also waits for the
// guest to retire this many instructions since the last one, so forward
// progress is guaranteed no matter how slow the host is; a tick is delayed,
// never denied. It costs nothing when the guest is keeping up, because a period
// normally buys far more instructions than this.
//
// The floor applies only while the CPU is executing. A guest parked in WFI
// retires nothing by definition, and the tick is what wakes it, so holding the
// tick back there would deadlock the machine -- the very thing the wall-clock
// timer was introduced to avoid.
//
// The value is set from the cost of one handler invocation with headroom: the
// kernel's scheduler.mln budgets its tick handler at "a few hundred
// instructions", and this leaves room for that plus the trampoline, the
// dispatcher and the serial/mouse drains.
const MIN_INSTRS_PER_TICK: u64 = 200_000;

pub struct TimerDevice {
    // Microseconds between ticks. None disables the timer.
    period: Option<Duration>,
    last_fire: Instant,
    instrs_at_last_fire: u64,
}

impl TimerDevice {
    pub fn new() -> Self {
        Self {
            period: Some(Duration::from_micros(1000)),
            last_fire: Instant::now(),
            instrs_at_last_fire: 0,
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
    pub fn service(&mut self, instrs_retired: u64, cpu_idle: bool) -> bool {
        if let Some(period) = self.period {
            let now = Instant::now();
            if now.duration_since(self.last_fire) >= period {
                // Hold the tick back until the guest has actually run. Without
                // this a handler that outlasts its period starves everything
                // else; see MIN_INSTRS_PER_TICK.
                if !cpu_idle
                    && instrs_retired.saturating_sub(self.instrs_at_last_fire) < MIN_INSTRS_PER_TICK
                {
                    return false;
                }
                self.last_fire += period;
                if now.duration_since(self.last_fire) > period * 32 {
                    self.last_fire = now;
                }
                self.instrs_at_last_fire = instrs_retired;
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
    use super::{TimerDevice, MIN_INSTRS_PER_TICK};
    use std::time::Duration;

    // Enough retired instructions that the floor never gets in the way; each
    // call advances the count by another whole quota.
    fn plenty(n: u64) -> u64 {
        n * MIN_INSTRS_PER_TICK
    }

    #[test]
    fn late_service_catches_up_instead_of_stretching_the_period() {
        let mut t = TimerDevice::new();
        t.set_period_micros(10_000); // 10 ms
        std::thread::sleep(Duration::from_millis(25));
        assert!(t.service(plenty(1), false), "first tick after a late poll");
        assert!(
            t.service(plenty(2), false),
            "the missed tick is delivered as an immediate catch-up tick"
        );
    }

    // A guest in WFI retires nothing; the tick has to fire anyway or it never
    // wakes up.
    #[test]
    fn an_idle_cpu_is_exempt_from_the_floor() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        std::thread::sleep(Duration::from_millis(5));
        assert!(t.service(0, true), "idle CPU still gets its tick");
    }

    #[test]
    fn disabled_timer_never_fires() {
        let mut t = TimerDevice::new();
        t.set_period_micros(0);
        assert!(!t.service(plenty(1), false));
        assert!(t.time_until_next().is_none());
    }

    // The floor is what stops a handler that outlasts its period from starving
    // the guest: the period alone is not enough to fire.
    #[test]
    fn a_tick_waits_for_the_guest_to_make_progress() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000); // 1 ms
        std::thread::sleep(Duration::from_millis(5));
        assert!(
            !t.service(MIN_INSTRS_PER_TICK - 1, false),
            "period elapsed, but the guest has not run its quota yet"
        );
        assert!(
            t.service(MIN_INSTRS_PER_TICK, false),
            "fires once the quota is met"
        );
    }
}
