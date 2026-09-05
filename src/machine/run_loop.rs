use std::io::Write;
use std::time::{Duration, Instant};

use crate::constants::{DISPLAY_HEIGHT, DISPLAY_REFRESH_HZ, DISPLAY_WIDTH};
use crate::instruction::{decode_instruction, mnemonic};

use super::{DebugOptions, Machine};

impl Machine {
    // Scan VRAM out to the window if at least one refresh interval has elapsed
    // since the last frame. Mirrors a hardware display controller refreshing at
    // a fixed rate independent of CPU speed. Returns false if the window was
    // closed (the caller should halt). No-op in headless mode. Input is NOT
    // sampled here — poll_input runs at its own, much faster cadence so brief
    // clicks between frames are still observed.
    pub(super) fn maybe_refresh_display(&mut self, force: bool) -> bool {
        if self.headless {
            return true;
        }
        let interval = Duration::from_nanos(1_000_000_000 / DISPLAY_REFRESH_HZ);
        if !force && self.last_frame.elapsed() < interval {
            return true;
        }
        if let Some(window) = &mut self.window {
            if !window.is_open() {
                return false;
            }
            // Scan out the front buffer once a program has swapped at least
            // once; otherwise present the back buffer directly so programs that
            // never double-buffer still show their drawing.
            let base = if self.swapped {
                &self.front
            } else {
                &self.vram
            };
            // Composite the hardware cursor over a copy of the frame. VRAM is
            // left untouched, so the guest never has to erase the old sprite.
            let scanout: &[u32] = if self.cursor_visible {
                if self.cursor_frame.len() != base.len() {
                    self.cursor_frame.resize(base.len(), 0);
                }
                self.cursor_frame.copy_from_slice(base);
                draw_cursor_sprite(
                    &mut self.cursor_frame,
                    self.cursor_x as i32,
                    self.cursor_y as i32,
                );
                &self.cursor_frame
            } else {
                base
            };
            let t0 = Instant::now();
            match self.shm.as_mut() {
                // Fast path: the server reads the frame out of shared memory,
                // so only a small request crosses the socket. poll_input()
                // already pumps the window on its own cadence, so presenting
                // must not call update() again -- that is the expensive half.
                Some(shm) => {
                    shm.present(scanout, DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32);
                }
                None => {
                    window
                        .update_with_buffer(scanout, DISPLAY_WIDTH, DISPLAY_HEIGHT)
                        .unwrap();
                }
            }
            if let Some(stats) = self.io_stats.as_mut() {
                stats.scanout_ns += t0.elapsed().as_nanos();
                stats.scanout_calls += 1;
                if let Some(prev) = stats.last_scanout {
                    let gap = prev.elapsed().as_nanos();
                    stats.scanout_gap_ns += gap;
                    if gap > stats.scanout_gap_max_ns {
                        stats.scanout_gap_max_ns = gap;
                    }
                }
                stats.last_scanout = Some(Instant::now());
            }
            self.last_frame = Instant::now();
        }
        true
    }

    // Pump the host window's event queue and sample the pointer, rate-limited
    // to MOUSE_POLL_MS. Every observed change lands in the device's event FIFO
    // and raises the mouse IRQ, so a press+release that both happen inside one
    // display frame still reach the guest as two events instead of vanishing.
    // Called on batch boundaries and WFI spins. No-op in headless mode.
    pub(super) fn poll_input(&mut self) {
        if self.headless {
            return;
        }
        if self.last_input_poll.elapsed() < Duration::from_millis(crate::constants::MOUSE_POLL_MS) {
            return;
        }
        self.last_input_poll = Instant::now();

        // window.update() drains the X event queue. Measured on this host it
        // costs ~0.5-1.2 ms, and running it at the 2 ms input cadence spent
        // over half the emulator's wall clock inside X, starving the guest CPU
        // -- the guest then completed only ~3 frames/s, which is what a cursor
        // lagging by hundreds of milliseconds actually was.
        //
        // Pump it at display cadence instead. minifb refreshes its cached
        // pointer inside update(), so the pointer is now sampled per frame
        // rather than every 2 ms; MOUSE_POLL_MS still gates how often the
        // cached value is latched into the device FIFO.
        //
        // (Querying the server directly at 2 ms was tried and is worse: on this
        // host XQueryPointer is a ~540 us round trip, so any per-2 ms X call is
        // too expensive. Getting sub-frame sampling back means having X push
        // motion events to us rather than polling it -- see MYOS-010.)
        // The button must be sampled at the fast cadence: a click shorter than
        // a frame has to reach the FIFO, and minifb only refreshes its cached
        // button state inside update(). Ask the server directly instead --
        // XQueryPointer is a round trip, but it runs only while a press is in
        // flight or once per frame, not on every poll.
        let button_now = self.shm.as_mut().and_then(|shm| shm.query_pointer());

        let pump_due = self.last_window_pump.elapsed()
            >= Duration::from_nanos(1_000_000_000 / DISPLAY_REFRESH_HZ);

        if pump_due {
            self.last_window_pump = Instant::now();
            if let Some(window) = &mut self.window {
                let t = Instant::now();
                window.update();
                let ns = t.elapsed().as_nanos();
                if let Some(stats) = self.io_stats.as_mut() {
                    stats.win_update_ns += ns;
                    stats.win_update_calls += 1;
                }
            }
        }

        let t_ms = Instant::now();
        let sampled = match button_now {
            // One round trip gives position and buttons together.
            Some((x, y, left)) => Some((
                x.clamp(0, DISPLAY_WIDTH as i32 - 1) as u32,
                y.clamp(0, DISPLAY_HEIGHT as i32 - 1) as u32,
                if left {
                    crate::constants::MOUSE_BUTTON_LEFT
                } else {
                    0
                },
            )),
            None => self.window.as_ref().and_then(|window| {
                let (mx, my) = window.get_mouse_pos(minifb::MouseMode::Clamp)?;
                // minifb reports raw window pixels. The WM may have scaled or
                // resized the window (WSLg HiDPI stretches it), in which case
                // raw pixels no longer match framebuffer coordinates and the
                // guest would see the cursor far off-screen. Normalize by the
                // actual window size; identity when the window is 1024x768.
                let (win_w, win_h) = window.get_size();
                let scale = |v: f32, win: usize, fb: usize| -> u32 {
                    let scaled = if win > 0 && win != fb {
                        v * fb as f32 / win as f32
                    } else {
                        v
                    };
                    (scaled as u32).min(fb as u32 - 1)
                };
                // Prefer the directly-queried button state; fall back to
                // minifb's cache when there is no X connection of our own.
                let buttons = match button_now {
                    Some((_, _, true)) => crate::constants::MOUSE_BUTTON_LEFT,
                    Some((_, _, false)) => 0,
                    None => {
                        if window.get_mouse_down(minifb::MouseButton::Left) {
                            crate::constants::MOUSE_BUTTON_LEFT
                        } else {
                            0
                        }
                    }
                };
                Some((
                    scale(mx, win_w, DISPLAY_WIDTH),
                    scale(my, win_h, DISPLAY_HEIGHT),
                    buttons,
                ))
            }),
        };
        if let Some(stats) = self.io_stats.as_mut() {
            stats.mouse_read_ns += t_ms.elapsed().as_nanos();
            stats.mouse_read_calls += 1;
        }

        if let Some((nx, ny, buttons)) = sampled {
            if self.mouse.update(nx, ny, buttons) {
                self.irq_cause |= crate::constants::IRQ_CAUSE_MOUSE;
                self.pending_irq = true;
            }
        }
    }
    // MYOS-004: advance the machine for up to max_instructions (or until it
    // halts, or a wall-clock safety cap elapses, whichever comes first) and
    // return. Used by control_stdio.rs to give the guest a chance to react to
    // an injected input (mouse event, "dom" keystroke) between commands.
    //
    // This mirrors execute_with_debug's core loop (poll devices, dispatch
    // IRQs, execute, sleep through WFI) but skips everything control_stdio
    // has no use for -- trace/profiler/breakpoint handling and the [STEP]
    // status prints, which would otherwise land on the same stdout the
    // client is reading command responses from. It intentionally does NOT
    // call start_serial_input: control_stdio owns stdin itself and injects
    // bytes via ingest_serial_bytes instead of the raw-forwarding thread.
    pub fn run_frame_budget(&mut self, max_instructions: u64) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_millis(50);
        let mut executed = 0u64;
        let mut batch_counter = 0u64;
        const BATCH_SIZE: u64 = 1_000;

        while !self.halted && executed < max_instructions {
            if Instant::now() >= deadline {
                break;
            }

            batch_counter += 1;
            if batch_counter >= BATCH_SIZE || self.waiting_for_interrupt {
                batch_counter = 0;
                self.poll_devices();
                self.poll_input(); // no-op headless
                if !self.maybe_refresh_display(false) {
                    self.halted = true;
                    break;
                }
            }

            self.try_dispatch_irq()?;

            if self.waiting_for_interrupt {
                let cap = Duration::from_millis(4);
                let wait = match self.timer.time_until_next() {
                    Some(until_tick) => until_tick.min(cap),
                    None => cap,
                };
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                continue;
            }

            let instruction = match self.fetch_instruction() {
                Ok(inst) => inst,
                Err(()) => continue,
            };
            self.program_counter = self.program_counter.wrapping_add(4);
            self.execute_instruction(instruction)?;
            executed += 1;
            self.instrs_retired = self.instrs_retired.wrapping_add(1);
        }

        Ok(())
    }

    pub fn set_instruction_pointer(&mut self, address: u32) {
        self.program_counter = address;
    }

    pub fn load_program(&mut self, program: &[u32], mut start_address: u32) {
        for instruction in program {
            self.bus_write(start_address, *instruction);
            start_address = start_address.wrapping_add(4);
        }
    }

    #[allow(dead_code)]
    pub fn execute(&mut self) -> Result<(), String> {
        self.execute_with_debug(DebugOptions::default())
    }

    pub fn execute_with_debug(&mut self, options: DebugOptions) -> Result<(), String> {
        self.start_serial_input();

        let mut executed_steps = 0u64;
        // Throttle expensive per-tick work (syscalls, serial polling, display
        // refresh) to once every BATCH_SIZE instructions. Checking the clock
        // and polling the serial channel on every single instruction was the
        // dominant bottleneck.
        const BATCH_SIZE: u64 = 1_000;
        let mut batch_counter: u64 = 0;
        while !self.halted {
            let current_pc = self.program_counter;
            if let Some(break_addr) = options.break_addr {
                if current_pc == break_addr {
                    println!("[BREAK] hit 0x{:08X}", current_pc);
                    self.print_registers();
                    return Ok(());
                }
            }

            // Poll the devices (timer/serial/SSD) and refresh the display
            // periodically. These are the expensive checks, so they run on
            // batch boundaries while executing; when idle (WFI) they run every
            // spin so the CPU wakes promptly.
            batch_counter += 1;
            if batch_counter >= BATCH_SIZE || self.waiting_for_interrupt {
                batch_counter = 0;
                // Exit cleanly on Ctrl+C / SIGTERM so the minifb Window is
                // dropped (destroying the X11 window) instead of orphaned.
                if crate::signals::requested() {
                    self.halted = true;
                    break;
                }
                self.poll_devices();
                self.poll_input();
                if !self.maybe_refresh_display(false) {
                    self.halted = true;
                    break;
                }
            }

            // Vector to a pending, enabled IRQ before the next fetch. This runs
            // every instruction (a two-boolean check on the fast path) so
            // dispatch latency is one instruction like real hardware, rather
            // than up to a whole polling batch; it also wakes the CPU out of
            // WFI the moment a cause is raised.
            self.try_dispatch_irq()?;

            // WFI: the CPU is idle until an interrupt fires. Rather than busy-spin
            // (burning host CPU and pinning a core), sleep the host thread until
            // the next timer tick is due, then loop back to service it. Real time
            // keeps advancing, so the timer still wakes the guest.
            if self.waiting_for_interrupt {
                // Under --step the run would otherwise hang here without ever
                // reaching the step limit (WFI executes no instructions), so
                // report the idle state and pause instead.
                if options.step_count.is_some() {
                    println!(
                        "[STEP] CPU idle (WFI) after {} instruction(s)",
                        executed_steps
                    );
                    self.print_registers();
                    return Ok(());
                }
                // Cap the sleep so display refresh and signals stay responsive.
                // With the timer disabled there is no next tick to wait for;
                // sleep the cap anyway instead of busy-spinning the host core
                // (serial input or the mouse can still wake the guest).
                let cap = Duration::from_millis(4);
                let wait = match self.timer.time_until_next() {
                    Some(until_tick) => until_tick.min(cap),
                    None => cap,
                };
                if !wait.is_zero() {
                    let t = Instant::now();
                    std::thread::sleep(wait);
                    if let Some(stats) = self.io_stats.as_mut() {
                        let ns = t.elapsed().as_nanos();
                        stats.wfi_sleep_ns += ns;
                        stats.wfi_sleeps += 1;
                        if ns > stats.wfi_sleep_max_ns {
                            stats.wfi_sleep_max_ns = ns;
                        }
                    }
                }
                continue;
            }

            let instruction = match self.fetch_instruction() {
                Ok(inst) => inst,
                Err(()) => continue,
            };
            self.program_counter = self.program_counter.wrapping_add(4);

            // Opcode is needed for both the trace print and the profiler; decode
            // once here so we don't decode twice on the hot path.
            let profiling = self.profiler.is_some();
            let opcode = if profiling || self.verbose || options.trace {
                decode_instruction(instruction).opcode
            } else {
                0
            };

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
            self.instrs_retired = self.instrs_retired.wrapping_add(1);

            // Feed the profiler after the instruction so program_counter and
            // link_register already reflect any jump/call it performed.
            if profiling {
                const OPCODE_CALL: u8 = 0x1B;
                let landed_pc = self.program_counter;
                let return_addr = self.link_register;
                if let Some(profiler) = self.profiler.as_mut() {
                    profiler.record_instruction(current_pc, opcode);
                    if opcode == OPCODE_CALL {
                        // The CALL set LR to its return address and PC to the
                        // callee entry; push a call-graph frame for it.
                        profiler.record_call(landed_pc, return_addr);
                    } else {
                        // Any other control flow (return via `mov pc, lr`, jump,
                        // fallthrough) may unwind one or more call frames.
                        profiler.record_control_flow(landed_pc);
                    }
                }
            }

            if let Some(step_limit) = options.step_count {
                if executed_steps >= step_limit {
                    println!("[STEP] paused after {} instruction(s)", executed_steps);
                    self.print_registers();
                    return Ok(());
                }
            }
        }

        // Force one final scan-out so the last frame is shown even if the loop
        // ended before the next refresh interval.
        self.maybe_refresh_display(true);
        Ok(())
    }
}

// The pointer sprite the display controller overlays: a small arrow with a
// white edge so it stays visible on any background. Shape mirrors the outline
// the guest used to draw for itself, but it is composited at scan-out and
// costs the guest nothing.
fn draw_cursor_sprite(frame: &mut [u32], hx: i32, hy: i32) {
    const BLACK: u32 = 0x0000_0000;
    const WHITE: u32 = 0x00FF_FFFF;
    // Rows of the arrow, from the hotspot down. 1 = outline, 2 = fill.
    const SPRITE: [&[u8]; 16] = [
        b"1",
        b"11",
        b"121",
        b"1221",
        b"12221",
        b"122221",
        b"1222221",
        b"12222221",
        b"122222221",
        b"1222222221",
        b"12222111111",
        b"122221",
        b"1221221",
        b"121 1221",
        b"11   1221",
        b"1     121",
    ];
    for (dy, row) in SPRITE.iter().enumerate() {
        let y = hy + dy as i32;
        if y < 0 || y >= DISPLAY_HEIGHT as i32 {
            continue;
        }
        for (dx, cell) in row.iter().enumerate() {
            let color = match cell {
                b'1' => BLACK,
                b'2' => WHITE,
                _ => continue,
            };
            let x = hx + dx as i32;
            if x < 0 || x >= DISPLAY_WIDTH as i32 {
                continue;
            }
            frame[y as usize * DISPLAY_WIDTH + x as usize] = color;
        }
    }
}
