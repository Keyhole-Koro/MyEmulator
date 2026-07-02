use std::io::Read;
use std::sync::mpsc;
use std::thread;

use crate::constants::{is_ram_address, IRQ_VECTOR_ADDR, SR_IE};

use super::Machine;

impl Machine {
    // Spawn a background thread that forwards stdin bytes to the RX queue.
    // Idempotent; safe to call before each run.
    pub(super) fn start_serial_input(&mut self) {
        if self.rx_recv.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel::<u8>();
        thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            let mut byte = [0u8; 1];
            loop {
                match lock.read(&mut byte) {
                    Ok(0) | Err(_) => break, // EOF or error
                    Ok(_) => {
                        if tx.send(byte[0]).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        self.rx_recv = Some(rx);
    }

    // Queue received bytes and raise an IRQ so the kernel's handler runs
    // (independent of the timer). Split out from polling so it is testable
    // without the stdin thread.
    pub(super) fn ingest_serial_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.rx_queue.extend(bytes.iter().copied());
        self.pending_irq = true;
    }

    // Drain any bytes the input thread has delivered into the RX queue.
    fn poll_serial_input(&mut self) {
        let mut arrived = Vec::new();
        if let Some(rx) = &self.rx_recv {
            while let Ok(b) = rx.try_recv() {
                arrived.push(b);
            }
        }
        self.ingest_serial_bytes(&arrived);
    }

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
        self.poll_serial_input();

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

#[cfg(test)]
mod tests {
    use super::Machine;
    use crate::constants::{
        IRQ_VECTOR_ADDR, SERIAL_LSR_ADDR, SERIAL_LSR_DR, SERIAL_LSR_THRE, SERIAL_RX_ADDR,
    };

    #[test]
    fn lsr_idle_reports_thre_only() {
        let m = Machine::new(false, true);
        // No input queued: transmitter ready, no data-ready bit.
        assert_eq!(m.bus_read(SERIAL_LSR_ADDR), SERIAL_LSR_THRE);
        assert_eq!(m.bus_read(SERIAL_LSR_ADDR) & SERIAL_LSR_DR, 0);
    }

    #[test]
    fn rx_register_consumes_fifo_in_order() {
        let mut m = Machine::new(false, true);
        m.ingest_serial_bytes(b"AB");
        assert_ne!(
            m.bus_read(SERIAL_LSR_ADDR) & SERIAL_LSR_DR,
            0,
            "data-ready set while a byte is queued"
        );
        assert_eq!(m.bus_load(SERIAL_RX_ADDR), u32::from(b'A'));
        assert_eq!(m.bus_load(SERIAL_RX_ADDR), u32::from(b'B'));
        assert_eq!(
            m.bus_read(SERIAL_LSR_ADDR) & SERIAL_LSR_DR,
            0,
            "data-ready clears once drained"
        );
        assert_eq!(m.bus_load(SERIAL_RX_ADDR), 0, "reading an empty RX yields 0");
    }

    #[test]
    fn received_input_raises_pending_irq() {
        let mut m = Machine::new(false, true);
        assert!(!m.pending_irq);
        m.ingest_serial_bytes(b"x");
        assert!(m.pending_irq);
    }

    #[test]
    fn rx_does_not_disturb_instruction_fetch() {
        // bus_read (used for fetch/stack) must NOT consume the RX byte; only the
        // LD instructions (bus_load) do.
        let mut m = Machine::new(false, true);
        m.ingest_serial_bytes(b"Z");
        let _ = m.bus_read(SERIAL_RX_ADDR);
        assert_eq!(m.bus_load(SERIAL_RX_ADDR), u32::from(b'Z'), "byte still there");
    }

    #[test]
    fn received_input_vectors_to_handler_when_enabled() {
        let mut m = Machine::new(false, true);
        let handler = 0x0000_0100u32; // a RAM address
        m.bus_write(IRQ_VECTOR_ADDR, handler);
        m.set_interrupt_enable(true);
        m.ingest_serial_bytes(b"z");
        m.service_timer_interrupt().unwrap();
        assert_eq!(m.program_counter, handler, "PC vectored to the handler");
        assert!(!m.interrupt_enable, "interrupts masked entering the handler");
    }

    #[test]
    fn no_input_does_not_interrupt() {
        let mut m = Machine::new(false, true);
        m.bus_write(IRQ_VECTOR_ADDR, 0x0000_0100);
        m.set_interrupt_enable(true);
        m.service_timer_interrupt().unwrap();
        assert_eq!(m.program_counter, 0, "no IRQ without a source");
    }
}
