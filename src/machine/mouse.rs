use std::collections::VecDeque;

use crate::constants::MOUSE_EVENT_QUEUE_DEPTH;

#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub x: u32,
    pub y: u32,
    pub buttons: u32,
}

pub struct MouseDevice {
    pub x: u32,
    pub y: u32,
    pub buttons: u32,
    // Hardware event FIFO: one entry per observed change. On overflow the
    // oldest entry is dropped so the newest state is always retained.
    events: VecDeque<MouseEvent>,
}

impl MouseDevice {
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            buttons: 0,
            events: VecDeque::with_capacity(MOUSE_EVENT_QUEUE_DEPTH),
        }
    }

    // Latch a host sample. Returns true (raise an IRQ) when the state changed.
    // The change is also queued as an event so the guest can observe every
    // transition — e.g. a press and release between two reads — rather than
    // just the latest state.
    pub fn update(&mut self, x: u32, y: u32, buttons: u32) -> bool {
        if x == self.x && y == self.y && buttons == self.buttons {
            return false;
        }
        self.x = x;
        self.y = y;
        self.buttons = buttons;
        if self.events.len() == MOUSE_EVENT_QUEUE_DEPTH {
            self.events.pop_front();
        }
        self.events.push_back(MouseEvent { x, y, buttons });
        true
    }

    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    // The head event. Reading an empty FIFO is a guest bug (EVT_STATUS said
    // nothing is queued); answering with the live state keeps it harmless.
    pub fn head_event(&self) -> MouseEvent {
        self.events.front().copied().unwrap_or(MouseEvent {
            x: self.x,
            y: self.y,
            buttons: self.buttons,
        })
    }

    pub fn pop_event(&mut self) {
        self.events.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::MouseDevice;
    use crate::constants::MOUSE_EVENT_QUEUE_DEPTH;

    #[test]
    fn press_and_release_between_reads_yield_two_events() {
        let mut m = MouseDevice::new();
        assert!(m.update(10, 20, 1), "press raises an IRQ");
        assert!(m.update(10, 20, 0), "release raises an IRQ");
        assert_eq!(m.event_count(), 2, "both transitions are queued");
        assert_eq!(m.head_event().buttons, 1, "press first");
        m.pop_event();
        assert_eq!(m.head_event().buttons, 0, "then release");
        m.pop_event();
        assert_eq!(m.event_count(), 0);
    }

    #[test]
    fn unchanged_sample_queues_nothing() {
        let mut m = MouseDevice::new();
        m.update(5, 5, 0);
        assert!(!m.update(5, 5, 0));
        assert_eq!(m.event_count(), 1);
    }

    #[test]
    fn overflow_drops_oldest_and_keeps_newest() {
        let mut m = MouseDevice::new();
        for i in 0..(MOUSE_EVENT_QUEUE_DEPTH as u32 + 8) {
            m.update(i, 0, 0);
        }
        assert_eq!(m.event_count(), MOUSE_EVENT_QUEUE_DEPTH as u32);
        assert_eq!(m.head_event().x, 8, "the oldest entries were dropped");
        // Drain to the tail: the newest state is still there.
        for _ in 0..MOUSE_EVENT_QUEUE_DEPTH - 1 {
            m.pop_event();
        }
        assert_eq!(m.head_event().x, MOUSE_EVENT_QUEUE_DEPTH as u32 + 7);
    }
}
