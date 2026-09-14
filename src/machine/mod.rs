use std::collections::HashMap;
use std::fs::File;
use std::time::Instant;

use crate::constants::RAM_SIZE;

mod cpu;
mod cpu_exec;
mod diagnostics;
mod display;
mod dma2d;
mod host_display;
mod interrupts;
mod keyboard;
mod memory_bus;
pub mod mmu;
mod mouse;
mod png;
mod profiler;
mod ram;
mod registers;
mod run_loop;
mod serial;
mod shm_present;
mod ssd;
mod stack;
mod timer;

#[cfg(test)]
mod regression_tests;

use diagnostics::IoStats;
use keyboard::KeyboardDevice;
use mmu::Mmu;
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
}

#[derive(Clone, Debug)]
pub struct IrqSnapshot {
    pub sp: u32,
    pub pc: u32,
    pub bp: u32,
    pub lr: u32,
    pub regs: [u32; 8],
    pub flags: (bool, bool, bool, bool),
    pub sr: u32,
    pub cause: u32,
}

pub struct Machine {
    cpu: cpu::Cpu,
    ram: ram::Ram,
    rom: Vec<u8>,
    io: HashMap<u32, u32>,
    display: display::DisplayDevice,
    host: host_display::HostDisplay,
    // Wall-clock cost of each host-side display/input layer, printed at exit
    // under --io-stats. Answers which layer a slow pointer actually comes from
    // rather than inferring it from guest instruction counts.
    io_stats: Option<IoStats>,
    // Persistent position within the 1000-step host/device service cadence.
    service_ticks: u16,

    verbose: bool,
    serial_log: Option<File>,
    // Host-only automation MMIO bridge. It is intentionally not represented
    // by the generic io map: RX consumes a request and TX is a byte stream.
    automation_request: Option<u32>,
    automation_tx_buf: Vec<u8>,
    automation_done: bool,
    control_stdio: bool,
    trace_log: Option<File>,
    // Instruction-level profiler. None unless --profile is passed; when present
    // the run loop and memory bus feed it one event per instruction/access.
    profiler: Option<Profiler>,

    // Interrupt state.
    pending_irq: bool,
    // Synchronous traps bypass IE, but are consumed once on entry. Their
    // diagnostic cause bits remain W1C and do not retrigger themselves.
    pending_trap: Option<u32>,
    irq_cause: u32,
    // Starvation diagnostics: wall-clock entry time of the in-flight timer IRQ
    // handler (closed by iret), how many consecutive handlers overran the tick
    // period, and whether the one-shot warning has been printed.
    timer_handler_entered: Option<Instant>,
    slow_timer_handler_streak: u32,
    starvation_warned: bool,
    // Debug (MYEMU_IRQ_CHECK=1): CPU state snapshot at IRQ entry, compared at
    // the iret that returns to the same stack depth. Reports any register or
    // flag the handler failed to restore.
    irq_check: Option<Vec<IrqSnapshot>>,
    // Debug (MYEMU_PC_RING=1): the last 64 program counters, printed with a
    // machine error so a crash can be traced back without a full trace log.
    pc_ring: Option<(Vec<u32>, usize)>,
    // Devices
    serial: SerialDevice,
    mouse: MouseDevice,
    keyboard: KeyboardDevice,
    timer: TimerDevice,
    ssd: SsdDevice,
    pub mmu: Mmu,
}

impl Machine {
    pub fn new(verbose: bool, headless: bool) -> Self {
        Self {
            cpu: cpu::Cpu::new(),
            ram: ram::Ram::new(RAM_SIZE as usize),
            rom: vec![0u8; crate::constants::ROM_SIZE as usize],
            io: HashMap::new(),
            display: display::DisplayDevice::new(),
            host: host_display::HostDisplay::new(headless),
            io_stats: None,
            service_ticks: 0,
            verbose,
            serial_log: None,
            automation_request: None,
            automation_tx_buf: Vec::new(),
            automation_done: false,
            control_stdio: false,
            trace_log: None,
            profiler: None,
            pending_irq: false,
            pending_trap: None,
            irq_cause: 0,
            timer_handler_entered: None,
            slow_timer_handler_streak: 0,
            starvation_warned: false,
            irq_check: if std::env::var("MYEMU_IRQ_CHECK").is_ok() {
                Some(Vec::new())
            } else {
                None
            },
            pc_ring: std::env::var("MYEMU_PC_RING").ok().map(|v| {
                (
                    vec![0u32; v.parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(64)],
                    0,
                )
            }),
            serial: SerialDevice::new(),
            mouse: MouseDevice::new(),
            keyboard: KeyboardDevice::new(),
            timer: TimerDevice::new(),
            ssd: SsdDevice::disabled(),
            mmu: Mmu::new(),
        }
    }

    #[inline]
    pub fn is_user_mode(&self) -> bool {
        (self.cpu.status_register & crate::constants::SR_USER) != 0
    }

    // Attach a host disk-image file as the SSD's backing store.
    pub fn load_disk(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.ssd = SsdDevice::load(path)?;
        Ok(())
    }

    // ROM loading path is not wired up yet; keep the API for the firmware boot flow.
    #[allow(dead_code)]
    pub fn load_rom(&mut self, binary: &[u32], start_address: u32) {
        for (i, &word) in binary.iter().enumerate() {
            let addr = start_address + (i as u32) * 4;
            let offset = (addr - crate::constants::ROM_START) as usize;
            if offset + 3 < self.rom.len() {
                let bytes = word.to_be_bytes();
                self.rom[offset..offset + 4].copy_from_slice(&bytes);
            }
        }
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

    pub fn set_timer_interval(&mut self, micros: u64) {
        self.timer.set_period_micros(micros);
    }

    pub fn enable_control_stdio(&mut self) {
        self.control_stdio = true;
    }

    pub fn automation_request(&mut self, command: u32) -> Result<(), String> {
        if self.automation_request.is_some() {
            return Err("automation request already in flight".to_string());
        }
        self.automation_request = Some(command);
        self.automation_tx_buf.clear();
        self.automation_done = false;
        Ok(())
    }

    pub fn automation_done(&self) -> bool {
        self.automation_done
    }

    pub fn take_automation_response(&mut self) -> Vec<u8> {
        self.automation_done = false;
        std::mem::take(&mut self.automation_tx_buf)
    }

    pub fn enable_io_stats(&mut self) {
        self.io_stats = Some(IoStats {
            run_started: Some(Instant::now()),
            ..IoStats::default()
        });
    }

    pub fn report_io_stats(&self) {
        if let Some(stats) = &self.io_stats {
            stats.report();
        }
    }
}
