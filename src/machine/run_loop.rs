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
            window
                .update_with_buffer(&self.vram, DISPLAY_WIDTH, DISPLAY_HEIGHT)
                .unwrap();
            self.last_frame = Instant::now();
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
        self.timer_interval = options.timer_interval;
        self.start_serial_input();

        let mut executed_steps = 0u64;
        while !self.halted {
            let current_pc = self.program_counter;
            if let Some(break_addr) = options.break_addr {
                if current_pc == break_addr {
                    println!("[BREAK] hit 0x{:08X}", current_pc);
                    self.print_registers();
                    return Ok(());
                }
            }

            self.service_timer_interrupt()?;

            let instruction = self.bus_read(self.program_counter);
            self.program_counter = self.program_counter.wrapping_add(4);

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

            if !self.maybe_refresh_display(false) {
                self.halted = true;
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
