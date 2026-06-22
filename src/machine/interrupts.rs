use crate::constants::{is_ram_address, IRQ_VECTOR_ADDR, SR_IE};

use super::Machine;

impl Machine {
    // Called by the EI/DI instructions. Keeps the SR's IE bit in sync with the
    // internal flag so the kernel/debugger can observe interrupt state.
    pub(super) fn set_interrupt_enable(&mut self, enable: bool) {
        self.interrupt_enable = enable;
        if enable {
            self.status_register |= SR_IE;
        } else {
            self.status_register &= !SR_IE;
        }
    }

    // Advances the timer and, when an interrupt is due and enabled, saves PC/SR
    // and vectors to the registered handler. Push order is PC then SR so the
    // handler's `iret` pops SR then PC (reverse order).
    pub(super) fn service_timer_interrupt(&mut self) -> Result<(), String> {
        if let Some(interval) = self.timer_interval {
            if interval > 0 {
                self.timer_counter = self.timer_counter.wrapping_add(1);
                if self.timer_counter >= interval {
                    self.timer_counter = 0;
                    self.pending_irq = true;
                }
            }
        }

        if self.interrupt_enable && self.pending_irq {
            let vector = self.bus_read(IRQ_VECTOR_ADDR);
            // Guard against an unregistered vector. The handler lives in RAM, so
            // a vector outside RAM (0, or the 0xFFFF_FFFF an unmapped I/O slot
            // reads back) means "no handler installed": leave the IRQ pending so
            // it fires once a real handler is registered.
            if is_ram_address(vector) {
                self.pending_irq = false;
                self.push(self.program_counter)?;
                self.push(self.status_register)?;
                // Disable further interrupts until the handler re-enables them
                // (prevents reentrant timer interrupts mid-handler).
                self.set_interrupt_enable(false);
                self.program_counter = vector;
            }
        }

        Ok(())
    }
}
