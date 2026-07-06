use std::io::Write;
use std::time::{Duration, Instant};

use crate::instruction::{decode_instruction, mnemonic};
use crate::constants::{DISPLAY_WIDTH, DISPLAY_HEIGHT, DISPLAY_REFRESH_HZ};

use super::{DebugOptions, Machine};

impl Machine {
    // Scan VRAM out to the window if at least one refresh interval has elapsed
    // since the last frame. Mirrors a hardware display controller refreshing at
    // a fixed rate independent of CPU speed. Returns false if the window was
    // closed (the caller should halt). No-op in headless mode.
    fn maybe_refresh_display(&mut self, force: bool) -> bool {
        if self.headless {
            return true;
        }
        let interval = Duration::from_nanos(1_000_000_000 / DISPLAY_REFRESH_HZ);
        if !force && self.last_frame.elapsed() < interval {
            return true;
        }
        if let Some(window) = &mut self.window {
            if !window.is_open() {
                return false;
            }
            // Scan out the front buffer once a program has swapped at least
            // once; otherwise present the back buffer directly so programs that
            // never double-buffer still show their drawing.
            let scanout = if self.swapped { &self.front } else { &self.vram };
            window
                .update_with_buffer(scanout, DISPLAY_WIDTH, DISPLAY_HEIGHT)
                .unwrap();
            self.last_frame = Instant::now();

            // Sample the host pointer. A change in position or button state
            // raises an IRQ so the kernel handler can poll the mouse registers.
            let (mx, my) = window
                .get_mouse_pos(minifb::MouseMode::Clamp)
                .unwrap_or((0.0, 0.0));
            let (nx, ny) = (mx as u32, my as u32);
            let buttons = if window.get_mouse_down(minifb::MouseButton::Left) {
                crate::constants::MOUSE_BUTTON_LEFT
            } else {
                0
            };
            if self.mouse.update(nx, ny, buttons) {
                self.irq_cause |= crate::constants::IRQ_CAUSE_MOUSE;
                self.pending_irq = true;
            }
        }
        true
    }
    pub fn set_instruction_pointer(&mut self, address: u32) {
        self.program_counter = address;
    }

    pub fn load_program(&mut self, program: &[u32], mut start_address: u32) {
        for instruction in program {
            self.bus_write(start_address, *instruction);
            start_address = start_address.wrapping_add(4);
        }
    }

    #[allow(dead_code)]
    pub fn execute(&mut self) -> Result<(), String> {
        self.execute_with_debug(DebugOptions::default())
    }

    pub fn execute_with_debug(&mut self, options: DebugOptions) -> Result<(), String> {
        self.timer.interval = options.timer_interval;
        self.start_serial_input();

        let mut executed_steps = 0u64;
        // Throttle expensive per-tick work (syscalls, serial polling, display
        // refresh) to once every BATCH_SIZE instructions. Checking the clock
        // and polling the serial channel on every single instruction was the
        // dominant bottleneck.
        const BATCH_SIZE: u64 = 1_000;
        let mut batch_counter: u64 = 0;
        while !self.halted {
            let current_pc = self.program_counter;
            if let Some(break_addr) = options.break_addr {
                if current_pc == break_addr {
                    println!("[BREAK] hit 0x{:08X}", current_pc);
                    self.print_registers();
                    return Ok(());
                }
            }

            // Only service timer/serial and refresh display every BATCH_SIZE
            // instructions to avoid per-instruction syscall overhead.
            batch_counter += 1;
            if batch_counter >= BATCH_SIZE {
                batch_counter = 0;
                self.service_timer_interrupt()?;
                if !self.maybe_refresh_display(false) {
                    self.halted = true;
                    break;
                }
            }

            let instruction = self.bus_read(self.program_counter);
            self.program_counter = self.program_counter.wrapping_add(4);

            // Opcode is needed for both the trace print and the profiler; decode
            // once here so we don't decode twice on the hot path.
            let profiling = self.profiler.is_some();
            let opcode = if profiling || self.verbose || options.trace {
                decode_instruction(instruction).opcode
            } else {
                0
            };

            if self.verbose || options.trace {
                let inst = decode_instruction(instruction);
                println!("------------------------------");
                let trace_line = format!(
                    "PC: 0x{:08X}, Instruction: 0x{:08X}, {} r1=0x{:X} r2=0x{:X} imm=0x{:X}",
                    current_pc,
                    instruction,
                    mnemonic(inst.opcode),
                    inst.reg1,
                    inst.reg2,
                    inst.imm
                );
                println!("{}", trace_line);
                if let Some(trace_log) = self.trace_log.as_mut() {
                    writeln!(trace_log, "------------------------------")
                        .map_err(|e| e.to_string())?;
                    writeln!(trace_log, "{}", trace_line).map_err(|e| e.to_string())?;
                }
            }

            self.execute_instruction(instruction)?;
            executed_steps = executed_steps.wrapping_add(1);

            // Feed the profiler after the instruction so program_counter and
            // link_register already reflect any jump/call it performed.
            if profiling {
                const OPCODE_CALL: u8 = 0x1B;
                let landed_pc = self.program_counter;
                let return_addr = self.link_register;
                if let Some(profiler) = self.profiler.as_mut() {
                    profiler.record_instruction(current_pc, opcode);
                    if opcode == OPCODE_CALL {
                        // The CALL set LR to its return address and PC to the
                        // callee entry; push a call-graph frame for it.
                        profiler.record_call(landed_pc, return_addr);
                    } else {
                        // Any other control flow (return via `mov pc, lr`, jump,
                        // fallthrough) may unwind one or more call frames.
                        profiler.record_control_flow(landed_pc);
                    }
                }
            }

            if let Some(step_limit) = options.step_count {
                if executed_steps >= step_limit {
                    println!("[STEP] paused after {} instruction(s)", executed_steps);
                    self.print_registers();
                    return Ok(());
                }
            }
        }

        // Force one final scan-out so the last frame is shown even if the loop
        // ended before the next refresh interval.
        self.maybe_refresh_display(true);
        Ok(())
    }
}
