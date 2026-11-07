use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::constants::RAM_END_EXCLUSIVE;

use super::Machine;
use std::time::Instant;

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
        if calls == 0 {
            0.0
        } else {
            total_ns as f64 / calls as f64 / 1000.0
        }
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
                if avg_gap_ms > 0.0 {
                    1000.0 / avg_gap_ms
                } else {
                    0.0
                },
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

impl Machine {
    pub(super) fn note_display_swap(&mut self) {
        let retired = self.cpu.instrs_retired;
        if let Some(stats) = self.io_stats.as_mut() {
            if stats.guest_swaps > 0 {
                let d = retired.saturating_sub(stats.instrs_at_last_swap);
                stats.instrs_per_frame_sum += d as u128;
                if d > stats.instrs_per_frame_max {
                    stats.instrs_per_frame_max = d;
                }
            }
            stats.instrs_at_last_swap = retired;
            stats.guest_swaps += 1;
            if let Some(prev) = stats.last_swap {
                let gap = prev.elapsed().as_nanos();
                stats.swap_gap_ns += gap;
                if gap > stats.swap_gap_max_ns {
                    stats.swap_gap_max_ns = gap;
                }
            }
            stats.last_swap = Some(std::time::Instant::now());
        }
    }

    pub fn set_serial_log<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        self.serial_log = Some(
            File::create(path_ref)
                .map_err(|e| format!("Unable to open serial log {}: {}", path_ref.display(), e))?,
        );
        Ok(())
    }

    // The buffer the display is showing: front if the guest has swapped at
    // least once, else the back buffer (mirrors maybe_refresh_display).
    fn scanout_pixels(&self) -> &[u32] {
        if self.devices.display.swapped {
            &self.devices.display.front
        } else {
            &self.devices.display.vram
        }
    }

    // Save the displayed frame. The extension picks the format: `.png` writes
    // a PNG (see png.rs), anything else a binary PPM (P6). Neither needs an
    // image crate. The hardware cursor is not composited in -- this is what
    // is in VRAM, which is what the guest's renderer is responsible for.
    pub fn write_screenshot<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let is_png = path
            .as_ref()
            .extension()
            .map(|e| e.eq_ignore_ascii_case("png"))
            .unwrap_or(false);
        if is_png {
            self.write_png_screenshot(path)
        } else {
            self.write_ppm_screenshot(path)
        }
    }

    pub fn write_png_screenshot<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        use crate::constants::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
        let file = File::create(&path).map_err(|e| {
            format!(
                "Unable to open screenshot {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        let mut out = std::io::BufWriter::new(file);
        super::png::write_png(
            &mut out,
            self.scanout_pixels(),
            DISPLAY_WIDTH,
            DISPLAY_HEIGHT,
        )
        .map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }

    // MYOS-004: dump the current scanout buffer as a binary PPM (P6): just a
    // header and raw RGB bytes.
    pub fn write_ppm_screenshot<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        use crate::constants::{DISPLAY_HEIGHT, DISPLAY_WIDTH};

        let base = self.scanout_pixels();
        let mut out = File::create(&path).map_err(|e| {
            format!(
                "Unable to open screenshot {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        write!(out, "P6\n{} {}\n255\n", DISPLAY_WIDTH, DISPLAY_HEIGHT)
            .map_err(|e| e.to_string())?;
        let mut rgb = Vec::with_capacity(DISPLAY_WIDTH * DISPLAY_HEIGHT * 3);
        for &pixel in base.iter().take(DISPLAY_WIDTH * DISPLAY_HEIGHT) {
            // 0x00RRGGBB, matching graphics.rgb() in the guest UI package.
            rgb.push(((pixel >> 16) & 0xFF) as u8);
            rgb.push(((pixel >> 8) & 0xFF) as u8);
            rgb.push((pixel & 0xFF) as u8);
        }
        out.write_all(&rgb).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn swap_count(&self) -> u64 {
        self.devices.display.swap_count
    }

    pub fn set_trace_log<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        self.execution.trace_log = Some(
            File::create(path_ref)
                .map_err(|e| format!("Unable to open trace log {}: {}", path_ref.display(), e))?,
        );
        Ok(())
    }

    pub fn dump_memory_text<P: AsRef<Path>>(
        &self,
        filename: P,
        start_address: u32,
        end_address: u32,
    ) -> Result<(), String> {
        if end_address >= RAM_END_EXCLUSIVE || start_address > end_address {
            return Err(format!(
                "Invalid memory dump range: 0x{start_address:08x}..0x{end_address:08x}"
            ));
        }

        let file = File::create(&filename).map_err(|e| {
            format!(
                "Unable to open dump file {}: {}",
                filename.as_ref().display(),
                e
            )
        })?;
        let mut out = BufWriter::new(file);

        writeln!(
            out,
            "Memory Dump from 0x{:x} to 0x{:x}",
            start_address, end_address
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            out,
            "------------------------------------------------------------"
        )
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
        out.flush().map_err(|e| e.to_string())?;

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
                let byte = self.bus_read_byte_physical(address.wrapping_add(i));
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

    pub fn register_report(&self) -> String {
        let mut report = String::new();
        for i in 0..=7 {
            let value = self.cpu.registers[i];
            report.push_str(&format!(
                "R{}: Unsigned={}, Signed={}, Binary={:032b}\n",
                i, value, value as i32, value
            ));
        }
        report.push_str(&format!("SP: {:x}\n", self.stack_pointer()));
        report.push_str(&format!("BP: {:x}\n", self.base_pointer()));
        report.push_str(&format!("PC: {:x}\n", self.program_counter()));
        report.push_str(&format!("SR: {:x}\n", self.status_register()));
        report.push_str(&format!("LR: {:x}\n", self.link_register()));
        report.push_str(&format!(
            "Flags: Carry={}, Zero={}, Sign={}, Overflow={}\n",
            self.carry_flag(),
            self.zero_flag(),
            self.sign_flag(),
            self.overflow_flag()
        ));
        report
    }

    pub fn register_report_hex(&self) -> String {
        let mut report = String::new();
        for i in 0..=7 {
            report.push_str(&format!("R{}: 0x{:08X}\n", i, self.cpu.registers[i]));
        }
        report.push_str(&format!("SP: 0x{:08X}\n", self.cpu.stack_pointer));
        report.push_str(&format!("BP: 0x{:08X}\n", self.cpu.base_pointer));
        report.push_str(&format!("PC: 0x{:08X}\n", self.cpu.program_counter));
        report.push_str(&format!("SR: 0x{:08X}\n", self.status_register()));
        report.push_str(&format!("LR: 0x{:08X}\n", self.cpu.link_register));
        report.push_str(&format!(
            "FLAGS: C={} Z={} S={} O={}\n",
            self.cpu.carry_flag, self.cpu.zero_flag, self.cpu.sign_flag, self.cpu.overflow_flag
        ));
        report
    }

    pub(super) fn debug_dump(&self) -> Result<(), String> {
        for i in 0..8 {
            println!("reg{}: 0x{:x}", i, self.cpu.registers[i]);
        }
        println!("PC: 0x{:x}", self.cpu.program_counter);
        println!("SP: 0x{:x}", self.cpu.stack_pointer);
        println!("BP: 0x{:x}", self.cpu.base_pointer);
        println!("SR: 0x{:x}", self.status_register());
        println!("LR: 0x{:x}", self.cpu.link_register);
        println!(
            "Flags: Zero: {}, Carry: {}, Sign: {}, Overflow: {}",
            self.cpu.zero_flag, self.cpu.carry_flag, self.cpu.sign_flag, self.cpu.overflow_flag
        );

        let filename = format!("memory_dump{}.txt", self.cpu.program_counter);
        let mut out = File::create(&filename)
            .map_err(|e| format!("Unable to open file {} for writing: {}", filename, e))?;

        let end_address: u32 = 0x0200_0000;
        let start_address: u32 = 0x2000_0000u32.wrapping_sub(0x0000_1000);
        let mut address = start_address;
        while address <= end_address {
            let value = self.bus_read_physical(address);
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
