use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs::File;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use minifb::{Window, WindowOptions};

use crate::constants::{RAM_END_EXCLUSIVE, RAM_START, DISPLAY_WIDTH, DISPLAY_HEIGHT, VRAM_SIZE};

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
    vram: Vec<u32>,

    window: Option<Window>,
    headless: bool,
    // Wall-clock time the display was last scanned out to the window. The
    // hardware display controller refreshes VRAM at a fixed rate regardless of
    // how many instructions the CPU has executed, so we drive updates off real
    // time rather than instruction counts.
    last_frame: Instant,

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

    // Serial input: a background thread reads stdin into this queue; arrivals
    // raise an IRQ. Reading SERIAL_RX_ADDR consumes from the front.
    rx_queue: VecDeque<u8>,
    rx_recv: Option<Receiver<u8>>,
}

impl Machine {
    pub fn new(verbose: bool, headless: bool) -> Self {
        let window = if !headless {
            let win = Window::new(
                "MyEmulator Display",
                DISPLAY_WIDTH,
                DISPLAY_HEIGHT,
                WindowOptions::default(),
            ).unwrap_or_else(|e| {
                panic!("{}", e);
            });
            // We gate scan-out ourselves on wall-clock time (see
            // maybe_refresh_display), so minifb's own rate limiter is left at
            // its default (off) to avoid blocking the CPU thread inside
            // update_with_buffer.
            Some(win)
        } else {
            None
        };

        Self {
            ram: HashMap::new(),
            io: HashMap::new(),
            vram: vec![0; (VRAM_SIZE / 4) as usize],
            window,
            headless,
            last_frame: Instant::now(),
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
            rx_queue: VecDeque::new(),
            rx_recv: None,
        }
    }
}
