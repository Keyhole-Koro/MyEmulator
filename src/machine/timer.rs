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

// The guest reads the timer as a clock of CLOCK_UNITS_PER_TICK units a tick:
// the ticks so far in the high bits, how far the current tick has got in the
// low ones. So the 32 bits hold 2^24 ticks before they wrap, and the tick
// count wraps with them.
pub const CLOCK_SHIFT: u32 = 8;
pub const CLOCK_UNITS_PER_TICK: u32 = 1 << CLOCK_SHIFT;
const TICK_MASK: u32 = u32::MAX >> CLOCK_SHIFT;

pub struct TimerDevice {
    // Microseconds between ticks. None disables the timer.
    period: Option<Duration>,
    last_fire: Instant,
    instrs_at_last_fire: u64,
    // Ticks so far, whether they interrupted or were held back, modulo 2^24:
    // the high bits of the clock read at TIMER_ADDR.
    count: u32,
    // From TIMER_ADDR's write: the ticks before this one pass without an
    // interrupt, and this one raises it. None: every tick interrupts.
    hold_until: Option<u32>,
}

impl TimerDevice {
    pub fn new() -> Self {
        Self {
            period: Some(Duration::from_micros(1000)),
            last_fire: Instant::now(),
            instrs_at_last_fire: 0,
            count: 0,
            hold_until: None,
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
                if let Some(until) = self.hold_until {
                    return self.service_held(until, period, now, instrs_retired);
                }
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
                self.count = self.count.wrapping_add(1) & TICK_MASK;
                return true;
            }
        }
        false
    }

    // While held, no interrupt is due before the wake tick, so the periods
    // that passed are counted together rather than one per call. A gap of
    // more than 32 periods means the emulator itself stood still (a debugger
    // pause, a control-stdio client between requests); as in the periodic
    // resync, it counts as a single tick. The wake tick interrupts even if
    // the guest has not run its MIN_INSTRS_PER_TICK: one interrupt cannot
    // starve it.
    fn service_held(
        &mut self,
        until: u32,
        period: Duration,
        now: Instant,
        instrs_retired: u64,
    ) -> bool {
        let passed = now.duration_since(self.last_fire).as_nanos() / period.as_nanos();
        let stood_still = passed > 32;
        let passed = if stood_still { 1 } else { passed as u32 };
        let ticks = passed.min(until.wrapping_sub(self.count) & TICK_MASK);
        self.count = self.count.wrapping_add(ticks) & TICK_MASK;
        if stood_still {
            self.last_fire = now;
        } else {
            self.last_fire += period * ticks;
        }
        if self.count != until {
            return false;
        }
        self.hold_until = None;
        self.instrs_at_last_fire = instrs_retired;
        true
    }

    // TIMER_ADDR read: the clock, the ticks so far and how far the current
    // tick has got, CLOCK_UNITS_PER_TICK units a tick. A tick that is due but
    // not yet counted (the instruction floor, a service still to come) holds
    // the clock at the last unit before it, so it never runs backwards.
    pub fn clock(&self) -> u32 {
        let within = self.period.map_or(0, |period| {
            let elapsed = Instant::now().duration_since(self.last_fire);
            let units = elapsed.as_nanos() * u128::from(CLOCK_UNITS_PER_TICK) / period.as_nanos();
            units.min(u128::from(CLOCK_UNITS_PER_TICK - 1)) as u32
        });
        (self.count << CLOCK_SHIFT) | within
    }

    // TIMER_ADDR write: interrupt next at the first tick at or after clock
    // `at`. 0, or a time no later than the next tick, interrupts at every
    // tick. The tick is compared within the clock's 2^24, so a hold reaches
    // at most 2^23 ticks ahead.
    pub fn hold(&mut self, at: u32) {
        let until = ((u64::from(at) + u64::from(CLOCK_UNITS_PER_TICK - 1)) >> CLOCK_SHIFT) as u32
            & TICK_MASK;
        let ahead = until.wrapping_sub(self.count) & TICK_MASK;
        self.hold_until = if at != 0 && ahead > 1 && ahead <= TICK_MASK >> 1 {
            Some(until)
        } else {
            None
        };
    }

    // How long until the next tick is due, or None when the timer is disabled.
    // Used to sleep the host thread while the guest is idle (WFI) instead of
    // busy-spinning.
    pub fn time_until_next(&self) -> Option<Duration> {
        self.period.map(|period| {
            let ticks = self.hold_until.map_or(1, |until| {
                (until.wrapping_sub(self.count) & TICK_MASK).max(1)
            });
            let elapsed = self.last_fire.elapsed();
            (period * ticks)
                .checked_sub(elapsed)
                .unwrap_or(Duration::ZERO)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CLOCK_SHIFT, CLOCK_UNITS_PER_TICK, MIN_INSTRS_PER_TICK, TICK_MASK, TimerDevice};
    use std::time::Duration;

    // The ticks the clock shows.
    fn ticks(t: &TimerDevice) -> u32 {
        t.clock() >> CLOCK_SHIFT
    }

    // The clock at the start of tick `n`.
    fn at(n: u32) -> u32 {
        n << CLOCK_SHIFT
    }

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
    fn every_tick_counts() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        std::thread::sleep(Duration::from_millis(3));
        assert!(t.service(plenty(1), false));
        assert_eq!(ticks(&t), 1, "a tick that interrupts counts");
    }

    // A held timer counts the ticks before the wake tick without
    // interrupting, all at once, and interrupts at the wake tick.
    #[test]
    fn held_ticks_count_without_interrupting_until_the_wake_tick() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.hold(at(ticks(&t) + 1_000));
        std::thread::sleep(Duration::from_millis(4));
        assert!(!t.service(0, true), "no interrupt before the wake tick");
        let counted = ticks(&t);
        assert!(
            counted >= 3,
            "the ticks that passed are counted at once: {counted}"
        );
        let wake = counted + 3;
        t.hold(at(wake));
        std::thread::sleep(Duration::from_millis(5));
        assert!(t.service(0, true), "the wake tick interrupts");
        assert_eq!(ticks(&t), wake, "and is the tick written");
        std::thread::sleep(Duration::from_millis(2));
        assert!(t.service(0, true), "every tick interrupts after it");
        assert_eq!(ticks(&t), wake + 1);
    }

    #[test]
    fn writing_zero_or_the_next_tick_interrupts_at_every_tick() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.hold(at(ticks(&t) + 50));
        t.hold(0);
        std::thread::sleep(Duration::from_millis(2));
        assert!(t.service(0, true), "0 cancels the hold");
        t.hold(at(ticks(&t) + 1));
        std::thread::sleep(Duration::from_millis(2));
        assert!(t.service(0, true), "the next tick is every tick anyway");
    }

    // A long gap between services means the emulator stood still; like the
    // periodic resync, it is one tick, not a jump in the guest's clock.
    #[test]
    fn a_held_timer_counts_a_standstill_as_one_tick() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.hold(at(ticks(&t) + 1_000));
        std::thread::sleep(Duration::from_millis(40));
        assert!(!t.service(0, true));
        assert_eq!(ticks(&t), 1);
    }

    #[test]
    fn a_held_timer_sleeps_until_its_wake_tick() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.hold(at(ticks(&t) + 20));
        let wait = t.time_until_next().unwrap();
        assert!(
            wait > Duration::from_millis(15),
            "the host may sleep past the held ticks: {wait:?}"
        );
    }

    // The clock shows how far the current tick has got, and never runs
    // backwards: not while a due tick waits to be counted, nor when it is.
    #[test]
    fn the_clock_shows_a_tick_in_progress() {
        let mut t = TimerDevice::new();
        t.set_period_micros(10_000); // 10 ms
        std::thread::sleep(Duration::from_millis(5));
        let half = t.clock();
        assert_eq!(half >> CLOCK_SHIFT, 0, "still the first tick");
        assert!(
            (64..CLOCK_UNITS_PER_TICK).contains(&half),
            "about half of it has gone: {half}"
        );
        std::thread::sleep(Duration::from_millis(10));
        let due = t.clock();
        assert_eq!(
            due,
            CLOCK_UNITS_PER_TICK - 1,
            "a due tick holds the clock at its last unit"
        );
        assert!(t.service(plenty(1), false));
        assert!(
            t.clock() >= due,
            "counting it moves the clock on: {}",
            t.clock()
        );
        assert_eq!(ticks(&t), 1);
    }

    // The ticks wrap at 2^24 with the clock, and a hold across the wrap still
    // wakes at its tick.
    #[test]
    fn a_hold_across_the_wrap_wakes_at_its_tick() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.count = TICK_MASK - 1;
        t.hold(at(TICK_MASK - 1).wrapping_add(at(10)));
        std::thread::sleep(Duration::from_millis(3));
        assert!(!t.service(0, true), "short of the wake tick");
        std::thread::sleep(Duration::from_millis(12));
        assert!(t.service(0, true), "the wake tick, past the wrap");
        assert_eq!(ticks(&t), 8, "TICK_MASK - 1 + 10 wraps to 8");
    }

    // A wake time too far ahead to tell from one in the past is no hold.
    #[test]
    fn a_hold_more_than_half_the_clock_ahead_is_ignored() {
        let mut t = TimerDevice::new();
        t.set_period_micros(1_000);
        t.hold(at(TICK_MASK / 2 + 10));
        std::thread::sleep(Duration::from_millis(2));
        assert!(t.service(0, true), "every tick interrupts");
    }

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
