use std::fs::File;
use std::time::Instant;

mod cpu;
mod cpu_exec;
mod devices;
mod diagnostics;
mod display;
mod dma2d;
mod execution_state;
mod guest_memory;
mod host_display;
mod interrupt_controller;
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

use devices::Devices;
use diagnostics::IoStats;
use execution_state::ExecutionState;
use guest_memory::GuestMemory;
use interrupt_controller::InterruptController;
use profiler::Profiler;
use ssd::SsdDevice;

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
    memory: GuestMemory,
    devices: Devices,
    host: host_display::HostDisplay,
    // Wall-clock cost of each host-side display/input layer, printed at exit
    // under --io-stats. Answers which layer a slow pointer actually comes from
    // rather than inferring it from guest instruction counts.
    io_stats: Option<IoStats>,
    // Host-only policy and observation state. Keeping it grouped prevents
    // tracing/profiling concerns from leaking into guest-device ownership.
    execution: ExecutionState,
    serial_log: Option<File>,
    // Synchronous traps bypass IE, but are consumed once on entry. Their
    // diagnostic cause bits remain W1C and do not retrigger themselves.
    interrupts: InterruptController,
}

impl Machine {
    pub fn new(verbose: bool, headless: bool) -> Self {
        Self {
            cpu: cpu::Cpu::new(),
            memory: GuestMemory::new(),
            devices: Devices::new(),
            host: host_display::HostDisplay::new(headless),
            io_stats: None,
            execution: ExecutionState::new(verbose),
            serial_log: None,
            interrupts: InterruptController::new(),
        }
    }

    #[inline]
    pub fn is_user_mode(&self) -> bool {
        (self.cpu.status_register & crate::constants::SR_USER) != 0
    }

    // Attach a host disk-image file as the SSD's backing store.
    pub fn load_disk(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.devices.ssd = SsdDevice::load(path)?;
        Ok(())
    }

    // ROM loading path is not wired up yet; keep the API for the firmware boot flow.
    #[allow(dead_code)]
    pub fn load_rom(&mut self, binary: &[u32], start_address: u32) {
        for (i, &word) in binary.iter().enumerate() {
            let addr = start_address + (i as u32) * 4;
            let offset = (addr - crate::constants::ROM_START) as usize;
            if offset + 3 < self.memory.rom.len() {
                let bytes = word.to_be_bytes();
                self.memory.rom[offset..offset + 4].copy_from_slice(&bytes);
            }
        }
    }

    // Turn on instruction-level profiling. `entry_pc` seeds the call graph's
    // outermost frame so instructions retired before the first CALL are still
    // attributed. `thread_var` is the kernel word holding the current thread, for
    // a program that has threads. Must be called before execution starts.
    pub fn enable_profiler(&mut self, entry_pc: u32, thread_var: Option<u32>) {
        self.execution.profiler = Some(Profiler::new(entry_pc, thread_var));
    }

    // Forget the counts so far, so the next write covers what runs from here.
    pub fn reset_profile(&mut self) -> bool {
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.reset();
            return true;
        }
        false
    }

    // Write the collected profile to `path` as JSON, leaving it running. No-op
    // (Ok) if profiling was never enabled.
    pub fn write_profile<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), String> {
        if let Some(profiler) = self.execution.profiler.as_ref() {
            profiler.write_json(path)?;
        }
        Ok(())
    }

    pub fn set_timer_interval(&mut self, micros: u64) {
        self.devices.timer.set_period_micros(micros);
    }

    pub fn enable_control_stdio(&mut self) {
        self.execution.control_stdio = true;
    }

    pub fn automation_request(&mut self, command: u32) -> Result<(), String> {
        if self.memory.automation.request.is_some() {
            return Err("automation request already in flight".to_string());
        }
        self.memory.automation.request = Some(command);
        self.memory.automation.tx_buf.clear();
        self.memory.automation.done = false;
        Ok(())
    }

    pub fn automation_done(&self) -> bool {
        self.memory.automation.done
    }

    pub fn take_automation_response(&mut self) -> Vec<u8> {
        self.memory.automation.done = false;
        std::mem::take(&mut self.memory.automation.tx_buf)
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
