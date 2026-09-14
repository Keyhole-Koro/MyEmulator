//! Guest interrupt/trap bookkeeping, separate from CPU register state.
use std::time::Instant;

use super::IrqSnapshot;

pub(super) struct InterruptController {
    pub pending_irq: bool,
    pub pending_trap: Option<u32>,
    pub cause: u32,
    pub timer_handler_entered: Option<Instant>,
    pub slow_timer_handler_streak: u32,
    pub starvation_warned: bool,
    pub irq_check: Option<Vec<IrqSnapshot>>,
}

impl InterruptController {
    pub fn new() -> Self {
        Self {
            pending_irq: false,
            pending_trap: None,
            cause: 0,
            timer_handler_entered: None,
            slow_timer_handler_streak: 0,
            starvation_warned: false,
            irq_check: std::env::var("MYEMU_IRQ_CHECK").is_ok().then(Vec::new),
        }
    }

    pub fn raise_irq(&mut self, cause: u32) {
        self.cause |= cause;
        self.pending_irq = true;
    }

    pub fn raise_trap(&mut self, cause: u32) {
        self.pending_trap = Some(cause);
    }

    pub fn acknowledge(&mut self, causes: u32) {
        self.cause &= !causes;
        if self.cause == 0 {
            self.pending_irq = false;
        }
    }
}
