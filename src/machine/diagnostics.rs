use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::constants::{RAM_END_EXCLUSIVE, RAM_START};

use super::Machine;

impl Machine {
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

    pub(super) fn debug_dump(&self) -> Result<(), String> {
        for i in 0..8 {
            println!("reg{}: 0x{:x}", i, self.registers[i]);
        }
        println!("PC: 0x{:x}", self.program_counter);
        println!("SP: 0x{:x}", self.stack_pointer);
        println!("BP: 0x{:x}", self.base_pointer);
        println!("SR: 0x{:x}", self.status_register);
        println!("LR: 0x{:x}", self.link_register);
        println!(
            "Flags: Zero: {}, Carry: {}, Sign: {}, Overflow: {}",
            self.zero_flag, self.carry_flag, self.sign_flag, self.overflow_flag
        );

        let filename = format!("memory_dump{}.txt", self.program_counter);
        let mut out = File::create(&filename)
            .map_err(|e| format!("Unable to open file {} for writing: {}", filename, e))?;

        let end_address: u32 = 0x0200_0000;
        let start_address: u32 = 0x2000_0000u32.wrapping_sub(0x0000_1000);
        let mut address = start_address;
        while address <= end_address {
            let value = self.bus_read(address);
            writeln!(out, "0x{:x}: 0x{:08x}", address, value).map_err(|e| e.to_string())?;
            if end_address - address < 4 {
                break;
            }
            address = address.wrapping_add(4);
        }

        println!("Memory dump written to {}", filename);
        Ok(())
    }
}
