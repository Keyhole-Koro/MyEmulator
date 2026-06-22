use crate::constants::{RAM_END_EXCLUSIVE, RAM_START};

use super::Machine;

impl Machine {
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

    pub(super) fn push(&mut self, value: u32) -> Result<(), String> {
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

    pub(super) fn pop(&mut self) -> Result<u32, String> {
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
