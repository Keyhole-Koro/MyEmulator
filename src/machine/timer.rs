pub struct TimerDevice {
    counter: u64,
    pub interval: Option<u64>,
}

impl TimerDevice {
    pub fn new() -> Self {
        Self { counter: 0, interval: None }
    }

    pub fn service(&mut self) -> bool {
        if let Some(interval) = self.interval {
            self.counter = self.counter.wrapping_add(1);
            if self.counter >= interval {
                self.counter = 0;
                return true;
            }
        }
        false
    }
}
