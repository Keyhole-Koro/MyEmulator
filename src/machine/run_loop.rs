use std::io::Write;

use crate::instruction::{decode_instruction, mnemonic};

use super::{DebugOptions, Machine};

impl Machine {
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

            if let Some(step_limit) = options.step_count {
                if executed_steps >= step_limit {
                    println!("[STEP] paused after {} instruction(s)", executed_steps);
                    self.print_registers();
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
