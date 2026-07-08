use std::io::Read;
use std::sync::mpsc;
use std::thread;

use crate::constants::{
    is_ram_address, IRQ_VECTOR_ADDR, SR_IE,
    IRQ_CAUSE_TIMER, IRQ_CAUSE_SERIAL
};

use super::Machine;

impl Machine {
    // Spawn a background thread that forwards stdin bytes to the RX queue.
    // Idempotent; safe to call before each run.
    pub(super) fn start_serial_input(&mut self) {
        if self.serial.has_rx_recv() {
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
        self.serial.set_rx_recv(rx);
    }

    // Queue received bytes and raise an IRQ so the kernel's handler runs
    // (independent of the timer). Split out from polling so it is testable
    // without the stdin thread.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn ingest_serial_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.serial.ingest_bytes(bytes);
        self.irq_cause |= IRQ_CAUSE_SERIAL;
        self.pending_irq = true;
    }

    // Drain any bytes the input thread has delivered into the RX queue.
    fn poll_serial_input(&mut self) {
        if self.serial.service_input() {
            self.irq_cause |= IRQ_CAUSE_SERIAL;
            self.pending_irq = true;
        }
    }

    // Update mouse state as if sampled from the host window, raising an IRQ on
    // any change (mirrors the real per-frame polling). Split out so it is
    // testable without a window.
    #[cfg(test)]
    pub(super) fn set_mouse_state(&mut self, x: u32, y: u32, buttons: u32) {
        use crate::constants::IRQ_CAUSE_MOUSE;
        if self.mouse.update(x, y, buttons) {
            self.irq_cause |= IRQ_CAUSE_MOUSE;
            self.pending_irq = true;
        }
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

        if self.timer.service() {
            self.irq_cause |= IRQ_CAUSE_TIMER;
            self.pending_irq = true;
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

    #[test]
    fn mouse_registers_reflect_state() {
        use crate::constants::{MOUSE_BUTTONS_ADDR, MOUSE_BUTTON_LEFT, MOUSE_X_ADDR, MOUSE_Y_ADDR};
        let mut m = Machine::new(false, true);
        m.set_mouse_state(320, 240, MOUSE_BUTTON_LEFT);
        assert_eq!(m.bus_read(MOUSE_X_ADDR), 320);
        assert_eq!(m.bus_read(MOUSE_Y_ADDR), 240);
        assert_eq!(m.bus_read(MOUSE_BUTTONS_ADDR), MOUSE_BUTTON_LEFT);
    }

    #[test]
    fn mouse_change_raises_irq() {
        let mut m = Machine::new(false, true);
        assert!(!m.pending_irq);
        m.set_mouse_state(10, 20, 0);
        assert!(m.pending_irq, "a mouse move raises an IRQ");
        m.pending_irq = false;
        m.set_mouse_state(10, 20, 0);
        assert!(!m.pending_irq, "no change means no new IRQ");
        m.set_mouse_state(10, 20, crate::constants::MOUSE_BUTTON_LEFT);
        assert!(m.pending_irq, "a button press raises an IRQ");
    }

    #[test]
    fn display_swap_copies_back_to_front() {
        use crate::constants::{DISPLAY_SWAP_ADDR, VRAM_BASE};
        let mut m = Machine::new(false, true);
        // Draw into the back buffer.
        m.bus_write(VRAM_BASE, 0x00AA_BBCC);
        assert!(!m.swapped, "no swap yet");
        assert_eq!(m.front[0], 0, "front untouched before swap");

        // Present: back -> front.
        m.bus_write(DISPLAY_SWAP_ADDR, 1);
        assert!(m.swapped, "swap latched");
        assert_eq!(m.front[0], 0x00AA_BBCC, "front now holds the drawn pixel");

        // Further drawing lands in back only, not shown until the next swap.
        m.bus_write(VRAM_BASE, 0x0011_2233);
        assert_eq!(m.front[0], 0x00AA_BBCC, "front unchanged until next swap");
        assert_eq!(m.bus_read(VRAM_BASE), 0x0011_2233, "back has the new pixel");
    }
}
