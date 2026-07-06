use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs::File;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use minifb::{Window, WindowOptions};

use crate::constants::{RAM_END_EXCLUSIVE, RAM_SIZE, RAM_START, DISPLAY_WIDTH, DISPLAY_HEIGHT, VRAM_SIZE};

mod cpu_exec;
mod diagnostics;
mod interrupts;
mod memory_bus;
mod mouse;
mod profiler;
mod registers;
mod run_loop;
mod serial;
mod ssd;
mod stack;
mod timer;

use mouse::MouseDevice;
use profiler::Profiler;
use serial::SerialDevice;
use ssd::SsdDevice;
use timer::TimerDevice;

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
    ram: Vec<u8>,
    io: HashMap<u32, u32>,
    // Back buffer: all VRAM writes (direct and DMA2D) land here.
    vram: Vec<u32>,
    // Front buffer: what the display scans out. A DISPLAY_SWAP copies vram into
    // this. Empty until the first swap, in which case the back buffer is shown
    // directly (see maybe_refresh_display).
    front: Vec<u32>,
    swapped: bool,

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
    // Instruction-level profiler. None unless --profile is passed; when present
    // the run loop and memory bus feed it one event per instruction/access.
    profiler: Option<Profiler>,

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
    irq_cause: u32,
    // Devices
    serial: SerialDevice,
    mouse: MouseDevice,
    timer: TimerDevice,
    ssd: SsdDevice,
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
            ram: vec![0u8; RAM_SIZE as usize],
            io: HashMap::new(),
            vram: vec![0; (VRAM_SIZE / 4) as usize],
            front: vec![0; (VRAM_SIZE / 4) as usize],
            swapped: false,
            window,
            headless,
            last_frame: Instant::now(),
            registers: [0; 8],
            verbose,
            serial_log: None,
            trace_log: None,
            profiler: None,
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
            irq_cause: 0,
            serial: SerialDevice::new(),
            mouse: MouseDevice::new(),
            timer: TimerDevice::new(),
            ssd: SsdDevice::disabled(),
        }
    }

    // Attach a host disk-image file as the SSD's backing store.
    pub fn load_disk(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.ssd = SsdDevice::load(path)?;
        Ok(())
    }

    // Turn on instruction-level profiling. `entry_pc` seeds the call graph's
    // outermost frame so instructions retired before the first CALL are still
    // attributed. Must be called before execution starts.
    pub fn enable_profiler(&mut self, entry_pc: u32) {
        self.profiler = Some(Profiler::new(entry_pc));
    }

    // Flush the collected profile to `path` as JSON. No-op (Ok) if profiling was
    // never enabled.
    pub fn write_profile<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        if let Some(profiler) = self.profiler.as_mut() {
            profiler.write_json(path)?;
        }
        Ok(())
    }
}
