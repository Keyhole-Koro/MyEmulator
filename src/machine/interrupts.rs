use std::io::Read;
use std::sync::mpsc;
use std::thread;

use crate::constants::{IRQ_CAUSE_SERIAL, IRQ_CAUSE_TIMER, IRQ_VECTOR_ADDR, SR_IE, is_ram_address};

use super::Machine;

impl Machine {
    // Spawn a background thread that forwards stdin bytes to the RX queue.
    // Idempotent; safe to call before each run.
    pub(super) fn start_serial_input(&mut self) {
        if self.devices.serial.has_rx_recv() {
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
        self.devices.serial.set_rx_recv(rx);
    }

    // Queue received bytes and raise an IRQ so the kernel's handler runs
    // (independent of the timer). Split out from polling so it is testable
    // without the stdin thread; control_stdio.rs also uses this directly to
    // inject synthetic keystrokes (e.g. typing "dom\r" at the shell) without
    // going through the raw-stdin-forwarding thread.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ingest_serial_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.devices.serial.ingest_bytes(bytes);
        self.interrupts.raise_irq(IRQ_CAUSE_SERIAL);
    }

    // Drain any bytes the input thread has delivered into the RX queue.
    fn poll_serial_input(&mut self) {
        if self.devices.serial.service_input() {
            self.interrupts.raise_irq(IRQ_CAUSE_SERIAL);
        }
    }

    // Update mouse state as if sampled from the host window, raising an IRQ on
    // any change (mirrors the real per-frame polling). Split out so it is
    // testable without a window; control_stdio.rs also calls this directly to
    // inject mouse.move/down/up commands in place of host window sampling.
    pub fn set_mouse_state(&mut self, x: u32, y: u32, buttons: u32) {
        self.set_mouse_state_wheel(x, y, buttons, 0);
    }

    pub fn set_mouse_state_wheel(&mut self, x: u32, y: u32, buttons: u32, wheel: i32) {
        if self.devices.mouse.update_with_wheel(x, y, buttons, wheel) {
            self.interrupts.raise_irq(crate::constants::IRQ_CAUSE_MOUSE);
        }
    }

    // Queue one keyboard event and raise the keyboard IRQ. poll_input feeds
    // host keystrokes through here; control_stdio.rs injects key.* commands.
    pub fn push_key_event(&mut self, kind: u32, code: u32, mods: u32) {
        if self
            .devices
            .keyboard
            .push(super::keyboard::KeyEvent { kind, code, mods })
        {
            self.interrupts
                .raise_irq(crate::constants::IRQ_CAUSE_KEYBOARD);
        }
    }

    // Called by the iret instruction. Closes the timer-handler runtime
    // measurement opened in try_dispatch_irq: if handlers keep taking longer
    // than the tick period, the guest can never make forward progress between
    // interrupts (total starvation), so warn once.
    pub(super) fn note_handler_return(&mut self) {
        let Some(entered) = self.interrupts.timer_handler_entered.take() else {
            return;
        };
        let Some(period) = self.devices.timer.period() else {
            return;
        };
        if entered.elapsed() >= period {
            self.interrupts.slow_timer_handler_streak += 1;
            if self.interrupts.slow_timer_handler_streak >= 16 && !self.interrupts.starvation_warned
            {
                self.interrupts.starvation_warned = true;
                eprintln!(
                    "[myemu] warning: timer IRQ handler exceeded the {}us tick period \
                     16 times in a row; the guest CPU is likely starving \
                     (reduce handler cost or raise --timer-interval)",
                    period.as_micros()
                );
            }
        } else {
            self.interrupts.slow_timer_handler_streak = 0;
        }
    }

    // Called by the EI/DI instructions. Keeps the SR's IE bit in sync with the
    // internal flag so the kernel/debugger can observe interrupt state.
    pub(super) fn set_interrupt_enable(&mut self, enable: bool) {
        self.cpu.interrupt_enable = enable;
        if enable {
            self.cpu.status_register |= SR_IE;
        } else {
            self.cpu.status_register &= !SR_IE;
        }
    }

    // Poll the interrupt sources: drain serial input, advance the timer, and
    // complete any in-flight SSD transfer. Sets cause bits and pending_irq but
    // does not vector; try_dispatch_irq does that on the next instruction
    // boundary. Called on batch boundaries and while idle in WFI.
    pub(super) fn poll_devices(&mut self) {
        self.poll_serial_input();

        if self
            .devices
            .timer
            .service(self.cpu.instrs_retired, self.cpu.waiting_for_interrupt)
        {
            self.interrupts.raise_irq(IRQ_CAUSE_TIMER);
        }

        self.service_ssd();
    }

    // When an interrupt is pending and enabled, saves PC/SR and vectors to the
    // registered handler. Push order is PC then SR so the handler's `iret` pops
    // SR then PC (reverse order). Called before every instruction fetch: the
    // fast path is two boolean loads, so real-CPU dispatch latency (one
    // instruction) costs nothing measurable.
    #[inline]
    pub(super) fn try_dispatch_irq(&mut self) -> Result<(), String> {
        let trap = self.interrupts.pending_trap;
        if trap.is_none() && !(self.cpu.interrupt_enable && self.interrupts.pending_irq) {
            return Ok(());
        }
        let vector = self.bus_read_physical(IRQ_VECTOR_ADDR);
        if vector == 0 || !is_ram_address(vector) || !vector.is_multiple_of(4) {
            return if trap.is_some() {
                Err("Synchronous trap without a valid IRQ vector".to_string())
            } else {
                Ok(())
            };
        }

        let was_user = self.is_user_mode();
        let sr = self.status_register();
        let frame_top = if was_user {
            self.memory.mmu.kernel_sp
        } else {
            self.cpu.stack_pointer
        };
        let frame_sp = frame_top
            .checked_sub(8)
            .ok_or_else(|| "Invalid interrupt stack".to_string())?;

        // Translate the complete frame with kernel permissions before mutating
        // SP, KERNEL_SP, pending state or RAM. Failure is a fatal double fault.
        self.restore_status_register(sr & !crate::constants::SR_USER & !SR_IE);
        let prepared = (|| {
            let pc_addr = self.prepare_stack_word(frame_sp + 4, super::mmu::AccessType::Write)?;
            let sr_addr = self.prepare_stack_word(frame_sp, super::mmu::AccessType::Write)?;
            Ok::<_, super::cpu::CpuError>((pc_addr, sr_addr))
        })();
        self.restore_status_register(sr);
        let (pc_addr, sr_addr) = prepared.map_err(|e| format!("Cannot enter interrupt: {e}"))?;

        if let Some(cause) = trap {
            self.interrupts.cause |= cause;
        }
        if let Some(stack) = self.interrupts.irq_check.as_mut() {
            stack.push(super::IrqSnapshot {
                sp: self.cpu.stack_pointer,
                pc: self.cpu.program_counter,
                bp: self.cpu.base_pointer,
                lr: self.cpu.link_register,
                regs: self.cpu.registers,
                flags: (
                    self.cpu.carry_flag,
                    self.cpu.zero_flag,
                    self.cpu.sign_flag,
                    self.cpu.overflow_flag,
                ),
                sr,
                cause: self.interrupts.cause,
            });
        }
        self.write_prepared_word(pc_addr, self.cpu.program_counter);
        self.write_prepared_word(sr_addr, sr);
        if was_user {
            self.memory.mmu.kernel_sp = self.cpu.stack_pointer;
        }
        self.cpu.stack_pointer = frame_sp;
        self.restore_status_register(sr & !crate::constants::SR_USER & !SR_IE);
        self.interrupts.pending_trap = None;
        self.interrupts.pending_irq = false;
        self.cpu.waiting_for_interrupt = false;
        self.cpu.program_counter = vector;
        if self.interrupts.cause & IRQ_CAUSE_TIMER != 0 {
            self.interrupts.timer_handler_entered = Some(std::time::Instant::now());
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
        assert_eq!(m.bus_read_physical(SERIAL_LSR_ADDR), SERIAL_LSR_THRE);
        assert_eq!(m.bus_read_physical(SERIAL_LSR_ADDR) & SERIAL_LSR_DR, 0);
    }

    #[test]
    fn rx_register_consumes_fifo_in_order() {
        let mut m = Machine::new(false, true);
        m.ingest_serial_bytes(b"AB");
        assert_ne!(
            m.bus_read_physical(SERIAL_LSR_ADDR) & SERIAL_LSR_DR,
            0,
            "data-ready set while a byte is queued"
        );
        assert_eq!(m.bus_load(SERIAL_RX_ADDR).unwrap(), u32::from(b'A'));
        assert_eq!(m.bus_load(SERIAL_RX_ADDR).unwrap(), u32::from(b'B'));
        assert_eq!(
            m.bus_read_physical(SERIAL_LSR_ADDR) & SERIAL_LSR_DR,
            0,
            "data-ready clears once drained"
        );
        assert_eq!(
            m.bus_load(SERIAL_RX_ADDR).unwrap(),
            0,
            "reading an empty RX yields 0"
        );
    }

    #[test]
    fn received_input_raises_pending_irq() {
        let mut m = Machine::new(false, true);
        assert!(!m.interrupts.pending_irq);
        m.ingest_serial_bytes(b"x");
        assert!(m.interrupts.pending_irq);
    }

    #[test]
    fn rx_does_not_disturb_instruction_fetch() {
        // Debugger physical peeks must not consume RX; guest loads do.
        let mut m = Machine::new(false, true);
        m.ingest_serial_bytes(b"Z");
        let _ = m.bus_read_physical(SERIAL_RX_ADDR);
        assert_eq!(
            m.bus_load(SERIAL_RX_ADDR).unwrap(),
            u32::from(b'Z'),
            "byte still there"
        );
    }

    #[test]
    fn received_input_vectors_to_handler_when_enabled() {
        let mut m = Machine::new(false, true);
        let handler = 0x0000_0100u32; // a RAM address
        m.bus_write_physical(IRQ_VECTOR_ADDR, handler);
        m.set_interrupt_enable(true);
        m.ingest_serial_bytes(b"z");
        m.try_dispatch_irq().unwrap();
        assert_eq!(m.cpu.program_counter, handler, "PC vectored to the handler");
        assert!(
            !m.cpu.interrupt_enable,
            "interrupts masked entering the handler"
        );
    }

    #[test]
    fn pending_irq_vectors_on_next_instruction_not_batch_boundary() {
        use super::super::DebugOptions;
        let mut m = Machine::new(false, true);
        let handler = 0x0000_0200u32;
        const HALT: u32 = 0x3F << 26;
        const MOV_R1_R1: u32 = (0x01 << 26) | (1 << 21) | (1 << 16);
        m.bus_write_physical(IRQ_VECTOR_ADDR, handler);
        m.bus_write_physical(handler, HALT);
        for i in 0..4u32 {
            m.bus_write_physical(i * 4, MOV_R1_R1);
        }
        m.set_interrupt_enable(true);
        m.ingest_serial_bytes(b"x");
        // One step is enough: the IRQ must vector before the first fetch, so
        // the single executed instruction is the handler's HALT — well before
        // the old 1000-instruction polling batch would have dispatched it.
        let opts = DebugOptions {
            step_count: Some(1),
            ..Default::default()
        };
        m.execute_with_debug(opts).unwrap();
        assert!(
            m.cpu.halted,
            "the handler's HALT ran as the very next instruction"
        );
    }

    #[test]
    fn no_input_does_not_interrupt() {
        let mut m = Machine::new(false, true);
        m.bus_write_physical(IRQ_VECTOR_ADDR, 0x0000_0100);
        m.set_interrupt_enable(true);
        m.poll_devices();
        m.try_dispatch_irq().unwrap();
        assert_eq!(m.cpu.program_counter, 0, "no IRQ without a source");
    }

    #[test]
    fn mouse_registers_reflect_state() {
        use crate::constants::{MOUSE_BUTTON_LEFT, MOUSE_BUTTONS_ADDR, MOUSE_X_ADDR, MOUSE_Y_ADDR};
        let mut m = Machine::new(false, true);
        m.set_mouse_state(320, 240, MOUSE_BUTTON_LEFT);
        assert_eq!(m.bus_read_physical(MOUSE_X_ADDR), 320);
        assert_eq!(m.bus_read_physical(MOUSE_Y_ADDR), 240);
        assert_eq!(m.bus_read_physical(MOUSE_BUTTONS_ADDR), MOUSE_BUTTON_LEFT);
    }

    #[test]
    fn mouse_change_raises_irq() {
        let mut m = Machine::new(false, true);
        assert!(!m.interrupts.pending_irq);
        m.set_mouse_state(10, 20, 0);
        assert!(m.interrupts.pending_irq, "a mouse move raises an IRQ");
        m.interrupts.pending_irq = false;
        m.set_mouse_state(10, 20, 0);
        assert!(!m.interrupts.pending_irq, "no change means no new IRQ");
        m.set_mouse_state(10, 20, crate::constants::MOUSE_BUTTON_LEFT);
        assert!(m.interrupts.pending_irq, "a button press raises an IRQ");
    }

    #[test]
    fn display_swap_copies_back_to_front() {
        use crate::constants::{DISPLAY_SWAP_ADDR, VRAM_BASE};
        let mut m = Machine::new(false, true);
        // Draw into the back buffer.
        m.bus_write_physical(VRAM_BASE, 0x00AA_BBCC);
        assert!(!m.devices.display.swapped, "no swap yet");
        assert_eq!(m.devices.display.front[0], 0, "front untouched before swap");

        // Present: back -> front.
        m.bus_write_physical(DISPLAY_SWAP_ADDR, 1);
        assert!(m.devices.display.swapped, "swap latched");
        assert_eq!(
            m.devices.display.front[0], 0x00AA_BBCC,
            "front now holds the drawn pixel"
        );

        // Further drawing lands in back only, not shown until the next swap.
        m.bus_write_physical(VRAM_BASE, 0x0011_2233);
        assert_eq!(
            m.devices.display.front[0], 0x00AA_BBCC,
            "front unchanged until next swap"
        );
        assert_eq!(
            m.bus_read_physical(VRAM_BASE),
            0x0011_2233,
            "back has the new pixel"
        );
    }

    #[test]
    fn irq_entry_pushes_live_flags_not_stale_sr_bits() {
        use crate::constants::{IRQ_VECTOR_ADDR, SR_IE, SR_ZERO};
        let mut m = Machine::new(false, true);
        // A previous restore left ZERO set in the mirrored SR field ...
        m.restore_status_register(SR_IE | SR_ZERO);
        // ... but the ALU has since cleared the live flag.
        m.cpu.zero_flag = false;
        m.bus_write_physical(IRQ_VECTOR_ADDR, 0x1000);
        m.interrupts.cause = crate::constants::IRQ_CAUSE_TIMER;
        m.interrupts.pending_irq = true;
        m.try_dispatch_irq().unwrap();
        let pushed_sr = m.bus_read_physical(m.cpu.stack_pointer);
        assert_eq!(pushed_sr & SR_ZERO, 0, "stale ZERO bit must not be pushed");
        assert_ne!(
            pushed_sr & SR_IE,
            0,
            "IE at entry is recorded in the pushed SR"
        );
    }
}
