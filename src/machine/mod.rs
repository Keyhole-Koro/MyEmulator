use std::collections::HashMap;
use std::fs::File;
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
mod shm_present;
mod ssd;
mod stack;
mod timer;

use mouse::MouseDevice;
use profiler::Profiler;
use serial::SerialDevice;
use shm_present::ShmPresenter;
use ssd::SsdDevice;
use timer::TimerDevice;

#[derive(Clone, Copy, Default)]
pub struct DebugOptions {
    pub trace: bool,
    pub break_addr: Option<u32>,
    pub step_count: Option<u64>,
}

// Wall-clock accounting for the host-side display/input path. Each field is the
// total time spent inside one layer plus how many times it ran, so the per-call
// cost of window.update(), update_with_buffer() and the pointer read can be
// compared directly against the cadence each is supposed to keep.
#[derive(Default)]
pub struct IoStats {
    pub run_started: Option<Instant>,
    pub win_update_ns: u128,
    pub win_update_calls: u64,
    pub scanout_ns: u128,
    pub scanout_calls: u64,
    pub mouse_read_ns: u128,
    pub mouse_read_calls: u64,
    // Gap between successive scan-outs: what the guest's frames actually get,
    // as opposed to the 60 Hz the refresh interval nominally asks for.
    pub scanout_gap_ns: u128,
    pub scanout_gap_max_ns: u128,
    pub last_scanout: Option<Instant>,
    // End-to-end pointer latency: how long a sampled pointer position waits in
    // the device FIFO before the guest pops it. A queue that keeps growing
    // means the guest cannot drain events as fast as the host produces them,
    // which shows up as a cursor lagging behind by a wall-clock delay rather
    // than by a frame.
    pub evt_queue_depth_sum: u128,
    pub evt_queue_depth_max: u32,
    pub evt_queue_samples: u64,
    pub evt_latency_ns: u128,
    pub evt_latency_max_ns: u128,
    pub evt_latency_samples: u64,
    pub evt_dropped: u64,
    // Rolling window so latency can be printed live while the pointer is being
    // moved, instead of only as an average at exit.
    pub live_window_start: Option<Instant>,
    pub live_lat_ns: u128,
    pub live_lat_max_ns: u128,
    pub live_samples: u64,
    // How often the guest finishes a frame (writes DISPLAY_SWAP). The scan-out
    // can only ever show the newest completed frame, so if the guest presents
    // rarely the cursor shows a stale position no matter how fast the host is.
    pub guest_swaps: u64,
    pub last_swap: Option<Instant>,
    pub swap_gap_ns: u128,
    pub swap_gap_max_ns: u128,
    // Where the emulator's wall clock actually goes while the guest is idle:
    // how long each WFI sleep lasted and how many there were. A guest that
    // presents rarely but uses almost no CPU is being paced by these sleeps.
    pub wfi_sleep_ns: u128,
    pub wfi_sleeps: u64,
    pub wfi_sleep_max_ns: u128,
    // Guest instructions retired between one present and the next. Separates
    // "the guest is computing a lot per frame" from "the guest is waiting":
    // combined with the frame interval it says whether a slow frame is CPU
    // work or idle time.
    pub instrs_at_last_swap: u64,
    pub instrs_per_frame_sum: u128,
    pub instrs_per_frame_max: u64,
}

impl IoStats {
    fn avg_us(total_ns: u128, calls: u64) -> f64 {
        if calls == 0 { 0.0 } else { total_ns as f64 / calls as f64 / 1000.0 }
    }

    pub fn report(&self) {
        let wall = self
            .run_started
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        eprintln!("\n=== host I/O timing (wall {:.2}s) ===", wall);
        eprintln!(
            "  window.update()      {:>8} calls  {:>8.3} ms total  {:>7.1} us/call",
            self.win_update_calls,
            self.win_update_ns as f64 / 1e6,
            Self::avg_us(self.win_update_ns, self.win_update_calls)
        );
        eprintln!(
            "  get_mouse_pos()      {:>8} calls  {:>8.3} ms total  {:>7.1} us/call",
            self.mouse_read_calls,
            self.mouse_read_ns as f64 / 1e6,
            Self::avg_us(self.mouse_read_ns, self.mouse_read_calls)
        );
        eprintln!(
            "  update_with_buffer() {:>8} calls  {:>8.3} ms total  {:>7.1} us/call",
            self.scanout_calls,
            self.scanout_ns as f64 / 1e6,
            Self::avg_us(self.scanout_ns, self.scanout_calls)
        );
        if self.scanout_calls > 1 {
            let gaps = self.scanout_calls - 1;
            let avg_gap_ms = self.scanout_gap_ns as f64 / gaps as f64 / 1e6;
            eprintln!(
                "  scan-out interval    {:>8.2} ms avg ({:.1} fps)   {:.2} ms worst",
                avg_gap_ms,
                if avg_gap_ms > 0.0 { 1000.0 / avg_gap_ms } else { 0.0 },
                self.scanout_gap_max_ns as f64 / 1e6
            );
        }
        if self.evt_queue_samples > 0 {
            eprintln!(
                "  mouse FIFO depth     {:>8.2} avg  {:>4} worst   ({} events dropped full)",
                self.evt_queue_depth_sum as f64 / self.evt_queue_samples as f64,
                self.evt_queue_depth_max,
                self.evt_dropped
            );
        }
        if self.evt_latency_samples > 0 {
            eprintln!(
                "  pointer->guest lag   {:>8.2} ms avg  {:>8.2} ms worst  (n={})",
                self.evt_latency_ns as f64 / self.evt_latency_samples as f64 / 1e6,
                self.evt_latency_max_ns as f64 / 1e6,
                self.evt_latency_samples
            );
        }
        if self.guest_swaps > 1 {
            let gaps = self.guest_swaps - 1;
            let ipf = self.instrs_per_frame_sum as f64 / gaps as f64;
            eprintln!(
                "  guest work/frame     {:>8.0} instrs  ({:.2} ms of CPU at 12.4 M/s)  {} worst",
                ipf,
                ipf / 12.4e6 * 1000.0,
                self.instrs_per_frame_max
            );
            let avg = self.swap_gap_ns as f64 / gaps as f64 / 1e6;
            eprintln!(
                "  guest frame (SWAP)   {:>8.2} ms avg ({:.1} fps)  {:.2} ms worst  n={}",
                avg,
                if avg > 0.0 { 1000.0 / avg } else { 0.0 },
                self.swap_gap_max_ns as f64 / 1e6,
                self.guest_swaps
            );
        }
        if self.wfi_sleeps > 0 {
            eprintln!(
                "  guest WFI sleep      {:>8} sleeps  {:>7.3} s total  {:>6.2} ms avg  {:.2} ms worst",
                self.wfi_sleeps,
                self.wfi_sleep_ns as f64 / 1e9,
                self.wfi_sleep_ns as f64 / self.wfi_sleeps as f64 / 1e6,
                self.wfi_sleep_max_ns as f64 / 1e6
            );
        }
        let host_total = self.win_update_ns + self.scanout_ns + self.mouse_read_ns;
        if wall > 0.0 {
            eprintln!(
                "  host I/O share       {:>8.1}% of wall clock",
                host_total as f64 / 1e9 / wall * 100.0
            );
        }
    }
}

pub struct Machine {
    // Sparse byte-addressed RAM keeps behavior while avoiding eager 512MB allocation.
    ram: Vec<u8>,
    rom: Vec<u8>,
    io: HashMap<u32, u32>,
    // Back buffer: all VRAM writes (direct and DMA2D) land here.
    vram: Vec<u32>,
    // Front buffer: what the display scans out. A DISPLAY_SWAP copies vram into
    // this. Empty until the first swap, in which case the back buffer is shown
    // directly (see maybe_refresh_display).
    front: Vec<u32>,
    swapped: bool,

    window: Option<Window>,
    // Fast scan-out path: blits frames from a shared memory segment the X
    // server reads directly. None when MIT-SHM is unavailable (remote display,
    // no libXext), in which case scan-out falls back to minifb.
    shm: Option<ShmPresenter>,
    // Hardware cursor state (CURSOR_* registers). Composited over the frame at
    // scan-out, so it never lands in VRAM and costs the guest nothing to move.
    cursor_x: u32,
    cursor_y: u32,
    cursor_visible: bool,
    // Scratch frame: the scanned-out image with the cursor drawn on top. Kept
    // as a field so a frame does not allocate.
    cursor_frame: Vec<u32>,
    headless: bool,
    // Wall-clock time the display was last scanned out to the window. The
    // hardware display controller refreshes VRAM at a fixed rate regardless of
    // how many instructions the CPU has executed, so we drive updates off real
    // time rather than instruction counts.
    last_frame: Instant,
    // Wall-clock time host input was last pumped/sampled (poll_input). Runs on
    // its own, faster cadence than the display so sub-frame clicks are caught.
    last_input_poll: Instant,
    // Wall-clock time the host window's event queue was last drained.
    last_window_pump: Instant,
    // Wall-clock cost of each host-side display/input layer, printed at exit
    // under --io-stats. Answers which layer a slow pointer actually comes from
    // rather than inferring it from guest instruction counts.
    io_stats: Option<IoStats>,
    // Total guest instructions retired, used to attribute CPU work per frame.
    instrs_retired: u64,

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
    waiting_for_interrupt: bool,
    irq_cause: u32,
    // Starvation diagnostics: wall-clock entry time of the in-flight timer IRQ
    // handler (closed by iret), how many consecutive handlers overran the tick
    // period, and whether the one-shot warning has been printed.
    timer_handler_entered: Option<Instant>,
    slow_timer_handler_streak: u32,
    starvation_warned: bool,
    // Devices
    serial: SerialDevice,
    mouse: MouseDevice,
    timer: TimerDevice,
    ssd: SsdDevice,
}

impl Machine {
    pub fn new(verbose: bool, headless: bool) -> Self {
        let window = if !headless {
            let mut win = Window::new(
                "MyEmulator Display",
                DISPLAY_WIDTH,
                DISPLAY_HEIGHT,
                WindowOptions::default(),
            ).unwrap_or_else(|e| {
                panic!("{}", e);
            });
            // We gate scan-out ourselves on wall-clock time (see
            // maybe_refresh_display) and pump input at MOUSE_POLL_MS (see
            // poll_input), so minifb's own rate limiter must be OFF. Its
            // default is a hidden 4 ms sleep inside every update()/
            // update_with_buffer() call, which would stall the CPU thread on
            // each input poll and starve the guest.
            win.set_target_fps(0);
            Some(win)
        } else {
            None
        };

        // Attach the fast scan-out path to the window minifb just created.
        // minifb keeps handling the window and its input; only the per-frame
        // pixel transfer moves off the socket. None => fall back to minifb.
        let shm = window.as_ref().and_then(|w| {
            ShmPresenter::new(
                w.get_window_handle(),
                DISPLAY_WIDTH as u32,
                DISPLAY_HEIGHT as u32,
            )
        });

        Self {
            ram: vec![0u8; RAM_SIZE as usize],
            rom: vec![0u8; crate::constants::ROM_SIZE as usize],
            io: HashMap::new(),
            vram: vec![0; (VRAM_SIZE / 4) as usize],
            front: vec![0; (VRAM_SIZE / 4) as usize],
            swapped: false,
            window,
            shm,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: false,
            cursor_frame: Vec::new(),
            headless,
            last_frame: Instant::now(),
            last_input_poll: Instant::now(),
            last_window_pump: Instant::now(),
            io_stats: None,
            instrs_retired: 0,
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
            waiting_for_interrupt: false,
            irq_cause: 0,
            timer_handler_entered: None,
            slow_timer_handler_streak: 0,
            starvation_warned: false,
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
