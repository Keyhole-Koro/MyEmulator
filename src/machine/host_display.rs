//! Window-system integration and input polling, outside CPU execution.
use super::{
    Machine,
    keyboard::{HostKeyCallback, HostKeyQueue},
    shm_present::ShmPresenter,
};
use crate::constants::{DISPLAY_HEIGHT, DISPLAY_REFRESH_HZ, DISPLAY_WIDTH};
use minifb::{Window, WindowOptions};
use std::time::{Duration, Instant};

pub(super) struct HostDisplay {
    pub window: Option<Window>,
    pub shm: Option<ShmPresenter>,
    pub headless: bool,
    pub last_frame: Instant,
    pub last_input_poll: Instant,
    pub last_window_pump: Instant,
    pub cursor_frame: Vec<u32>,
    pub host_keys: HostKeyQueue,
}
impl Machine {
    // Scan VRAM out to the window if at least one refresh interval has elapsed
    // since the last frame. Mirrors a hardware display controller refreshing at
    // a fixed rate independent of CPU speed. Returns false if the window was
    // closed (the caller should halt). No-op in headless mode. Input is NOT
    // sampled here — poll_input runs at its own, much faster cadence so brief
    // clicks between frames are still observed.
    pub(super) fn maybe_refresh_display(&mut self, force: bool) -> bool {
        if self.host.headless {
            return true;
        }
        let interval = Duration::from_nanos(1_000_000_000 / DISPLAY_REFRESH_HZ);
        if !force && self.host.last_frame.elapsed() < interval {
            return true;
        }
        if let Some(window) = &mut self.host.window {
            if !window.is_open() {
                return false;
            }
            // Scan out the front buffer once a program has swapped at least
            // once; otherwise present the back buffer directly so programs that
            // never double-buffer still show their drawing.
            let base = if self.devices.display.swapped {
                &self.devices.display.front
            } else {
                &self.devices.display.vram
            };
            // Composite the hardware cursor over a copy of the frame. VRAM is
            // left untouched, so the guest never has to erase the old sprite.
            let scanout: &[u32] = if self.devices.display.cursor_visible {
                if self.host.cursor_frame.len() != base.len() {
                    self.host.cursor_frame.resize(base.len(), 0);
                }
                self.host.cursor_frame.copy_from_slice(base);
                draw_cursor_sprite(
                    &mut self.host.cursor_frame,
                    self.devices.display.cursor_x as i32,
                    self.devices.display.cursor_y as i32,
                );
                &self.host.cursor_frame
            } else {
                base
            };
            let t0 = Instant::now();
            match self.host.shm.as_mut() {
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
            self.host.last_frame = Instant::now();
        }
        true
    }

    // Pump the host window's event queue and sample the pointer, rate-limited
    // to MOUSE_POLL_MS. Every observed change lands in the device's event FIFO
    // and raises the mouse IRQ, so a press+release that both happen inside one
    // display frame still reach the guest as two events instead of vanishing.
    // Called on batch boundaries and WFI spins. No-op in headless mode.
    pub(super) fn poll_input(&mut self) {
        if self.host.headless {
            return;
        }
        if self.host.last_input_poll.elapsed()
            < Duration::from_millis(crate::constants::MOUSE_POLL_MS)
        {
            return;
        }
        self.host.last_input_poll = Instant::now();

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
        let button_now = self.host.shm.as_mut().and_then(|shm| shm.query_pointer());

        let pump_due = self.host.last_window_pump.elapsed()
            >= Duration::from_nanos(1_000_000_000 / DISPLAY_REFRESH_HZ);

        // Wheel motion and key events only surface inside window.update(), so
        // they are collected on the pump cadence and attached to the sample
        // taken right after.
        let mut wheel_steps: i32 = 0;
        if pump_due {
            self.host.last_window_pump = Instant::now();
            if let Some(window) = &mut self.host.window {
                let t = Instant::now();
                window.update();
                let ns = t.elapsed().as_nanos();
                if let Some(stats) = self.io_stats.as_mut() {
                    stats.win_update_ns += ns;
                    stats.win_update_calls += 1;
                }
                if let Some((_, dy)) = window.get_scroll_wheel() {
                    // minifb reports X11 wheel clicks as +-1.0 per notch.
                    wheel_steps = dy.round() as i32;
                }
            }
            // Keystrokes the input callback collected during update().
            let drained: Vec<_> = self.host.host_keys.borrow_mut().drain(..).collect();
            for ev in drained {
                self.push_key_event(ev.kind, ev.code, ev.mods);
            }
        }

        let t_ms = Instant::now();
        let sampled = match button_now {
            // One round trip gives position and buttons together.
            Some((x, y, buttons)) => Some((
                x.clamp(0, DISPLAY_WIDTH as i32 - 1) as u32,
                y.clamp(0, DISPLAY_HEIGHT as i32 - 1) as u32,
                buttons,
            )),
            None => self.host.window.as_ref().and_then(|window| {
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
                // No X connection of our own: fall back to minifb's cache.
                let mut buttons = 0;
                if window.get_mouse_down(minifb::MouseButton::Left) {
                    buttons |= crate::constants::MOUSE_BUTTON_LEFT;
                }
                if window.get_mouse_down(minifb::MouseButton::Right) {
                    buttons |= crate::constants::MOUSE_BUTTON_RIGHT;
                }
                if window.get_mouse_down(minifb::MouseButton::Middle) {
                    buttons |= crate::constants::MOUSE_BUTTON_MIDDLE;
                }
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
            if self
                .devices
                .mouse
                .update_with_wheel(nx, ny, buttons, wheel_steps)
            {
                self.interrupts.raise_irq(crate::constants::IRQ_CAUSE_MOUSE);
            }
        }
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

impl HostDisplay {
    pub fn new(headless: bool) -> Self {
        let mut window = if !headless {
            let mut win = Window::new(
                "MyEmulator Display",
                DISPLAY_WIDTH,
                DISPLAY_HEIGHT,
                WindowOptions::default(),
            )
            .unwrap_or_else(|e| {
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

        let host_keys: HostKeyQueue =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
        if let Some(win) = window.as_mut() {
            win.set_input_callback(Box::new(HostKeyCallback::new(host_keys.clone())));
        }

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
            window,
            shm,
            headless,
            host_keys,
            cursor_frame: Vec::new(),
            last_frame: Instant::now(),
            last_input_poll: Instant::now(),
            last_window_pump: Instant::now(),
        }
    }
}
