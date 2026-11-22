//! Window-system integration and input polling, outside CPU execution.
use super::{
    Machine,
    keyboard::{HostKeyCallback, HostKeyQueue},
    shm_present::ShmPresenter,
};
use crate::constants::{DISPLAY_HEIGHT, DISPLAY_REFRESH_HZ, DISPLAY_WIDTH};
use minifb::{Window, WindowOptions};
use std::time::{Duration, Instant};

// What the window was last sent: the guest's frame and the cursor over it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Scanout {
    swapped: bool,
    swap_count: u64,
    cursor: Option<(u32, u32, u32)>, // x, y, shape while visible
}

// How long a window may go without a present when nothing changes, so that
// a part the server lost (the window was covered) comes back.
const KEEPALIVE: Duration = Duration::from_millis(500);

pub(super) struct HostDisplay {
    pub window: Option<Window>,
    pub shm: Option<ShmPresenter>,
    pub headless: bool,
    pub presented: Option<(Scanout, Instant)>,
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
            let display = &self.devices.display;
            let now = Scanout {
                swapped: display.swapped,
                swap_count: display.swap_count,
                cursor: display
                    .cursor_visible
                    .then_some((display.cursor_x, display.cursor_y, display.cursor_shape)),
            };
            let base = if display.swapped { &display.front } else { &display.vram };
            // Before the first swap every refresh may hold new pixels; after it
            // only a swap does. The cursor is ours to draw, so its own moves
            // are known exactly.
            let last = self.host.presented;
            let frame_changed = force
                || !now.swapped
                || last.is_none_or(|(was, _)| was.swapped != now.swapped || was.swap_count != now.swap_count);
            let cursor_changed = last.is_none_or(|(was, _)| was.cursor != now.cursor);
            let stale = last.is_none_or(|(_, at)| at.elapsed() >= KEEPALIVE);
            if !frame_changed && !cursor_changed && !stale {
                self.host.last_frame = Instant::now();
                return true;
            }
            let t0 = Instant::now();
            let kind = match self.host.shm.as_mut() {
                // Fast path: the server reads the frame out of shared memory,
                // so only a small request crosses the socket. poll_input()
                // already pumps the window on its own cadence, so presenting
                // must not call update() again -- that is the expensive half.
                Some(shm) => {
                    // Still reading the last put: try again at the next tick,
                    // with nothing recorded as presented.
                    if !shm.ready() {
                        if let Some(stats) = self.io_stats.as_mut() {
                            stats.present_busy += 1;
                        }
                        return true;
                    }
                    let pixels = shm.pixels();
                    if frame_changed {
                        pixels.copy_from_slice(&base[..pixels.len()]);
                        if let Some((x, y, shape)) = now.cursor {
                            draw_cursor_sprite(pixels, x as i32, y as i32, shape);
                        }
                        shm.put(0, 0, DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32);
                        PresentKind::Frame
                    } else if cursor_changed {
                        // Only the cursor moved: put back the pixels under the
                        // old sprite, draw the new one, send the two.
                        let mut area = Rect::EMPTY;
                        if let Some((was, _)) = last {
                            if let Some((x, y, shape)) = was.cursor {
                                let under = cursor_rect(x as i32, y as i32, shape);
                                restore(pixels, base, under);
                                area = area.union(under);
                            }
                        }
                        if let Some((x, y, shape)) = now.cursor {
                            draw_cursor_sprite(pixels, x as i32, y as i32, shape);
                            area = area.union(cursor_rect(x as i32, y as i32, shape));
                        }
                        shm.put(area.x, area.y, area.w, area.h);
                        PresentKind::Cursor
                    } else {
                        // Nothing new: send the segment again as it is.
                        shm.put(0, 0, DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32);
                        PresentKind::Again
                    }
                }
                None => {
                    // minifb takes a whole frame: composite the hardware
                    // cursor over a copy, leaving VRAM untouched.
                    let scanout: &[u32] = if let Some((x, y, shape)) = now.cursor {
                        if self.host.cursor_frame.len() != base.len() {
                            self.host.cursor_frame.resize(base.len(), 0);
                        }
                        self.host.cursor_frame.copy_from_slice(base);
                        draw_cursor_sprite(&mut self.host.cursor_frame, x as i32, y as i32, shape);
                        &self.host.cursor_frame
                    } else {
                        base
                    };
                    window
                        .update_with_buffer(scanout, DISPLAY_WIDTH, DISPLAY_HEIGHT)
                        .unwrap();
                    PresentKind::Frame
                }
            };
            self.host.presented = Some((now, Instant::now()));
            let instrs = self.cpu.instrs_retired;
            if let Some(timeline) = self.execution.timeline.as_mut() {
                let name = match kind {
                    PresentKind::Frame => "present frame",
                    PresentKind::Cursor => "present cursor",
                    PresentKind::Again => "present again",
                };
                timeline.span(super::timeline::EMULATOR, name, t0, instrs);
            }
            if let Some(stats) = self.io_stats.as_mut() {
                match kind {
                    PresentKind::Frame => stats.present_frames += 1,
                    PresentKind::Cursor => stats.present_cursor += 1,
                    PresentKind::Again => stats.present_again += 1,
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
                let instrs = self.cpu.instrs_retired;
                if let Some(timeline) = self.execution.timeline.as_mut() {
                    timeline.span(super::timeline::EMULATOR, "window events", t, instrs);
                }
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
// The pointer sprites the display controller overlays, composited at scan-out
// so they cost the guest nothing. Each row starts at the sprite's left edge;
// 1 = black, 2 = white, anything else is transparent.

// The arrow, its tip on the hotspot, with a white fill so it stays visible on
// any background. It mirrors the outline the guest used to draw for itself.
const ARROW: [&[u8]; 16] = [
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

// The I-beam shown over text: a black stroke with a white edge, centred on
// the hotspot.
const IBEAM: [&[u8]; 17] = [
    b"222 222",
    b"2112112",
    b"2221222",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"  212",
    b"2221222",
    b"2112112",
    b"222 222",
];

enum PresentKind {
    Frame,  // a new guest frame (and the cursor over it)
    Cursor, // only the cursor moved
    Again,  // nothing changed: the keepalive
}

// A rectangle of the display, clipped to it; w == 0 is empty.
#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    const EMPTY: Rect = Rect { x: 0, y: 0, w: 0, h: 0 };

    fn clipped(left: i32, top: i32, width: i32, height: i32) -> Rect {
        let x0 = left.clamp(0, DISPLAY_WIDTH as i32);
        let y0 = top.clamp(0, DISPLAY_HEIGHT as i32);
        let x1 = (left + width).clamp(0, DISPLAY_WIDTH as i32);
        let y1 = (top + height).clamp(0, DISPLAY_HEIGHT as i32);
        if x1 <= x0 || y1 <= y0 {
            return Rect::EMPTY;
        }
        Rect { x: x0 as u32, y: y0 as u32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 }
    }

    fn union(self, other: Rect) -> Rect {
        if self.w == 0 {
            return other;
        }
        if other.w == 0 {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

// Where a sprite's top-left goes for a cursor at (hx, hy), and the sprite.
fn cursor_sprite(hx: i32, hy: i32, shape: u32) -> (i32, i32, &'static [&'static [u8]]) {
    if shape == crate::constants::CURSOR_SHAPE_IBEAM {
        (hx - 3, hy - 8, &IBEAM)
    } else {
        (hx, hy, &ARROW)
    }
}

// The display pixels the cursor at (hx, hy) covers.
fn cursor_rect(hx: i32, hy: i32, shape: u32) -> Rect {
    let (left, top, sprite) = cursor_sprite(hx, hy, shape);
    let width = sprite.iter().map(|row| row.len()).max().unwrap_or(0);
    Rect::clipped(left, top, width as i32, sprite.len() as i32)
}

// Copies `area` of the frame back over whatever was drawn on top of it.
fn restore(target: &mut [u32], frame: &[u32], area: Rect) {
    for y in area.y..area.y + area.h {
        let start = y as usize * DISPLAY_WIDTH + area.x as usize;
        let end = start + area.w as usize;
        target[start..end].copy_from_slice(&frame[start..end]);
    }
}

fn draw_cursor_sprite(frame: &mut [u32], hx: i32, hy: i32, shape: u32) {
    let (left, top, sprite) = cursor_sprite(hx, hy, shape);
    draw_sprite(frame, left, top, sprite);
}

fn draw_sprite(frame: &mut [u32], left: i32, top: i32, sprite: &[&[u8]]) {
    const BLACK: u32 = 0x0000_0000;
    const WHITE: u32 = 0x00FF_FFFF;
    for (dy, row) in sprite.iter().enumerate() {
        let y = top + dy as i32;
        if y < 0 || y >= DISPLAY_HEIGHT as i32 {
            continue;
        }
        for (dx, cell) in row.iter().enumerate() {
            let color = match cell {
                b'1' => BLACK,
                b'2' => WHITE,
                _ => continue,
            };
            let x = left + dx as i32;
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
            presented: None,
            host_keys,
            cursor_frame: Vec::new(),
            last_frame: Instant::now(),
            last_input_poll: Instant::now(),
            last_window_pump: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The frame a full present makes: the guest's pixels with the cursor on top.
    fn composite(frame: &[u32], cursor: Option<(i32, i32, u32)>) -> Vec<u32> {
        let mut out = frame.to_vec();
        if let Some((x, y, shape)) = cursor {
            draw_cursor_sprite(&mut out, x, y, shape);
        }
        out
    }

    // Moving only the cursor -- restore under the old sprite, draw the new
    // one -- leaves the same pixels as compositing the whole frame again,
    // and touches nothing outside the rectangle it sends.
    #[test]
    fn a_cursor_move_matches_a_full_composite() {
        let frame: Vec<u32> = (0..DISPLAY_WIDTH * DISPLAY_HEIGHT).map(|i| (i as u32).wrapping_mul(2654435761)).collect();
        let moves = [
            ((100, 100, 0), (130, 90, 0)),
            ((100, 100, 0), (104, 103, crate::constants::CURSOR_SHAPE_IBEAM)),
            ((DISPLAY_WIDTH as i32 - 3, DISPLAY_HEIGHT as i32 - 2, 0), (0, 0, crate::constants::CURSOR_SHAPE_IBEAM)),
            ((-5, -5, 0), (2, 1, 0)),
        ];
        for ((ox, oy, os), (nx, ny, ns)) in moves {
            let mut shown = composite(&frame, Some((ox, oy, os)));
            let under = cursor_rect(ox, oy, os);
            restore(&mut shown, &frame, under);
            draw_cursor_sprite(&mut shown, nx, ny, ns);
            let sent = under.union(cursor_rect(nx, ny, ns));
            let expected = composite(&frame, Some((nx, ny, ns)));
            assert!(shown == expected, "move ({ox},{oy}) -> ({nx},{ny}) left stale pixels");
            // Everything that differs from the old screen lies inside `sent`.
            let before = composite(&frame, Some((ox, oy, os)));
            for (i, (a, b)) in before.iter().zip(expected.iter()).enumerate() {
                if a != b {
                    let (x, y) = ((i % DISPLAY_WIDTH) as u32, (i / DISPLAY_WIDTH) as u32);
                    assert!(x >= sent.x && x < sent.x + sent.w && y >= sent.y && y < sent.y + sent.h,
                            "pixel ({x},{y}) changed outside the rectangle sent");
                }
            }
        }
    }

    #[test]
    fn rectangles_clip_to_the_display_and_unite() {
        let r = Rect::clipped(-4, -4, 10, 10);
        assert_eq!((r.x, r.y, r.w, r.h), (0, 0, 6, 6));
        assert_eq!(Rect::clipped(DISPLAY_WIDTH as i32, 0, 10, 10).w, 0);
        let u = Rect::EMPTY.union(Rect::clipped(5, 6, 2, 3)).union(Rect::clipped(1, 2, 2, 2));
        assert_eq!((u.x, u.y, u.w, u.h), (1, 2, 6, 7));
    }
}
