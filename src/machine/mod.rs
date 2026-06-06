use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::constants::{RAM_END_EXCLUSIVE, RAM_START};
use crate::instruction::{decode_instruction, mnemonic};

mod cpu_exec;
mod memory_bus;

#[derive(Clone, Copy, Default)]
pub struct DebugOptions {
    pub trace: bool,
    pub break_addr: Option<u32>,
    pub step_count: Option<u64>,
}

pub struct Machine {
    // Sparse byte-addressed RAM keeps behavior while avoiding eager 512MB allocation.
    ram: HashMap<u32, u8>,
    io: HashMap<u32, u32>,

    registers: [u32; 8],
    verbose: bool,
    serial_log: Option<File>,
    trace_log: Option<File>,

    stack_pointer: u32,
    base_pointer: u32,
    program_counter: u32,
    status_register: u32,
    link_register: u32,

    halted: bool,
    carry_flag: bool,
    zero_flag: bool,
    sign_flag: bool,
    overflow_flag: bool,
}

impl Machine {
    pub fn new(verbose: bool) -> Self {
        Self {
            ram: HashMap::new(),
            io: HashMap::new(),
            registers: [0; 8],
            verbose,
            serial_log: None,
            trace_log: None,
            stack_pointer: RAM_END_EXCLUSIVE,
            base_pointer: 0,
            program_counter: RAM_START,
            status_register: 0,
            link_register: 0,
            halted: false,
            carry_flag: false,
            zero_flag: false,
            sign_flag: false,
            overflow_flag: false,
        }
    }

    pub fn set_instruction_pointer(&mut self, address: u32) {
        self.program_counter = address;
    }

    pub fn set_serial_log<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        self.serial_log = Some(File::create(path_ref).map_err(|e| {
            format!("Unable to open serial log {}: {}", path_ref.display(), e)
        })?);
        Ok(())
    }

    pub fn set_trace_log<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        self.trace_log = Some(File::create(path_ref).map_err(|e| {
            format!("Unable to open trace log {}: {}", path_ref.display(), e)
        })?);
        Ok(())
    }

    pub fn get_data_register(&self, index: usize) -> Result<u32, String> {
        self.registers
            .get(index)
            .copied()
            .ok_or_else(|| format!("Register index out of range: {}", index))
    }

    pub fn stack_pointer(&self) -> u32 {
        self.stack_pointer
    }

    pub fn base_pointer(&self) -> u32 {
        self.base_pointer
    }

    pub fn program_counter(&self) -> u32 {
        self.program_counter
    }

    pub fn status_register(&self) -> u32 {
        self.status_register
    }

    pub fn link_register(&self) -> u32 {
        self.link_register
    }

    pub fn carry_flag(&self) -> bool {
        self.carry_flag
    }

    pub fn zero_flag(&self) -> bool {
        self.zero_flag
    }

    pub fn sign_flag(&self) -> bool {
        self.sign_flag
    }

    pub fn overflow_flag(&self) -> bool {
        self.overflow_flag
    }

    pub fn load_program(&mut self, program: &[u32], mut start_address: u32) {
        for instruction in program {
            self.bus_write(start_address, *instruction);
            start_address = start_address.wrapping_add(4);
        }
    }

    pub fn execute(&mut self) -> Result<(), String> {
        self.execute_with_debug(DebugOptions::default())
    }

    pub fn execute_with_debug(&mut self, options: DebugOptions) -> Result<(), String> {
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

    pub fn display_stack(&self) {
        println!("Stack Contents:");

        if self.stack_pointer == RAM_END_EXCLUSIVE {
            println!("  [Stack is empty]");
            return;
        }

        let mut current_address = RAM_END_EXCLUSIVE;
        let max_entries = 100u32;
        let mut displayed = 0u32;

        while current_address > self.stack_pointer && displayed < max_entries {
            let value = self.read_stack_memory(current_address);
            println!(
                "  Address: 0x{:x} | Value: 0x{:x}",
                current_address, value
            );
            current_address = current_address.wrapping_sub(4);
            displayed += 1;
        }

        if displayed == max_entries {
            println!("  [Output truncated: more entries exist]");
        }
    }

    pub fn dump_memory_text<P: AsRef<Path>>(
        &self,
        filename: P,
        start_address: u32,
        end_address: u32,
    ) -> Result<(), String> {
        if start_address < RAM_START || end_address >= RAM_END_EXCLUSIVE || start_address > end_address {
            eprintln!("Error: Invalid memory range for dump.");
            return Ok(());
        }

        let mut out = File::create(&filename)
            .map_err(|e| format!("Unable to open dump file {}: {}", filename.as_ref().display(), e))?;

        writeln!(
            out,
            "Memory Dump from 0x{:x} to 0x{:x}",
            start_address, end_address
        )
        .map_err(|e| e.to_string())?;
        writeln!(out, "------------------------------------------------------------")
            .map_err(|e| e.to_string())?;

        let mut address = start_address;
        while address <= end_address {
            let value = self.ram_read_word(address);
            writeln!(out, "0x{:x}: 0x{:08x}", address, value).map_err(|e| e.to_string())?;

            if end_address - address < 4 {
                break;
            }
            address = address.wrapping_add(4);
        }

        println!("Memory dump written to {}", filename.as_ref().display());
        Ok(())
    }

    pub fn dump_memory_range(&self, start_address: u32, length: u32) {
        if length == 0 {
            println!("[MEM] empty range");
            return;
        }

        let end_address = start_address.saturating_add(length.saturating_sub(1));
        println!(
            "[MEM] 0x{:08X}..0x{:08X} ({} byte(s))",
            start_address, end_address, length
        );

        let mut address = start_address;
        let mut remaining = length;
        while remaining > 0 {
            print!("0x{:08X}:", address);
            let line_bytes = remaining.min(16);
            for i in 0..line_bytes {
                let byte = self.bus_read_byte(address.wrapping_add(i)) as u8;
                print!(" {:02X}", byte);
            }
            println!();
            address = address.wrapping_add(line_bytes);
            remaining -= line_bytes;
        }
    }

    pub fn print_registers(&self) {
        print!("{}", self.register_report_hex());
    }

    pub fn register_report_hex(&self) -> String {
        let mut report = String::new();
        for i in 0..=7 {
            report.push_str(&format!("R{}: 0x{:08X}\n", i, self.registers[i]));
        }
        report.push_str(&format!("SP: 0x{:08X}\n", self.stack_pointer));
        report.push_str(&format!("BP: 0x{:08X}\n", self.base_pointer));
        report.push_str(&format!("PC: 0x{:08X}\n", self.program_counter));
        report.push_str(&format!("SR: 0x{:08X}\n", self.status_register));
        report.push_str(&format!("LR: 0x{:08X}\n", self.link_register));
        report.push_str(&format!(
            "FLAGS: C={} Z={} S={} O={}\n",
            self.carry_flag, self.zero_flag, self.sign_flag, self.overflow_flag
        ));
        report
    }

    fn get_register(&self, reg: u8) -> Result<u32, String> {
        match reg {
            0x00..=0x07 => Ok(self.registers[reg as usize]),
            0x08 => Ok(self.program_counter),
            0x09 => Ok(self.stack_pointer),
            0x0A => Ok(self.base_pointer),
            0x0B => Ok(self.status_register),
            0x0C => Ok(self.link_register),
            _ => Err(format!("Invalid register index {}", reg)),
        }
    }

    fn set_register(&mut self, reg: u8, value: u32) -> Result<(), String> {
        match reg {
            0x00..=0x07 => self.registers[reg as usize] = value,
            0x08 => self.program_counter = value,
            0x09 => self.stack_pointer = value,
            0x0A => self.base_pointer = value,
            0x0B => self.status_register = value,
            0x0C => self.link_register = value,
            _ => return Err(format!("Invalid register index {}", reg)),
        }
        Ok(())
    }

    fn update_zero_flag(&mut self, value: u32) {
        self.zero_flag = value == 0;
    }

    fn push(&mut self, value: u32) -> Result<(), String> {
        if self.stack_pointer == RAM_START {
            return Err("Stack overflow".to_string());
        }
        self.stack_pointer = self.stack_pointer.wrapping_sub(4);
        self.bus_write(self.stack_pointer, value);

        if self.verbose {
            let seen = self.bus_read(self.stack_pointer.wrapping_add(4));
            println!(
                "read {} from stack at address: 0x{:x}",
                seen,
                self.stack_pointer.wrapping_add(4)
            );
        }

        Ok(())
    }

    fn pop(&mut self) -> Result<u32, String> {
        if self.stack_pointer > RAM_END_EXCLUSIVE {
            return Err(format!(
                "Stack underflow at address: 0x{:x} (STACKBASE: 0x{:x})",
                self.stack_pointer, RAM_END_EXCLUSIVE
            ));
        }

        let value = self.bus_read(self.stack_pointer);
        self.stack_pointer = self.stack_pointer.wrapping_add(4);
        Ok(value)
    }
}
