use std::collections::HashMap;
use std::fs::File;

use crate::constants::{RAM_END_EXCLUSIVE, RAM_START};

mod cpu_exec;
mod diagnostics;
mod interrupts;
mod memory_bus;
mod registers;
mod run_loop;
mod stack;

#[derive(Clone, Copy, Default)]
pub struct DebugOptions {
    pub trace: bool,
    pub break_addr: Option<u32>,
    pub step_count: Option<u64>,
    // When set, raise a timer interrupt every N executed instructions.
    pub timer_interval: Option<u64>,
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

    // Interrupt state.
    interrupt_enable: bool,
    pending_irq: bool,
    timer_counter: u64,
    timer_interval: Option<u64>,
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
            interrupt_enable: false,
            pending_irq: false,
            timer_counter: 0,
            timer_interval: None,
        }
    }
}
