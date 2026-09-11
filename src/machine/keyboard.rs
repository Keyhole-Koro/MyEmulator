// Keyboard device: a FIFO of host key events exposed through the KBD_EVT_*
// registers (constants.rs). Two kinds of host input feed it, both delivered by
// minifb's InputCallback during window.update(): raw key presses/releases
// (KBD_EVT_DOWN/UP with a KEY_* code) and translated characters
// (KBD_EVT_CHAR with the code point, already shifted/localised by the host).
// Both go into the one queue in arrival order, so the guest sees "key down
// 'a', char 'A', key up 'a'" for a shifted keystroke and can pick whichever
// it needs: text fields consume CHAR events, shortcuts look at DOWN events.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use minifb::{InputCallback, Key};

use crate::constants::{
    KBD_EVENT_QUEUE_DEPTH, KBD_EVT_CHAR, KBD_EVT_DOWN, KBD_EVT_UP, KBD_MOD_ALT, KBD_MOD_CTRL,
    KBD_MOD_SHIFT, KEY_ALT, KEY_BACKSPACE, KEY_CTRL, KEY_DELETE, KEY_DOWN, KEY_END, KEY_ENTER,
    KEY_ESCAPE, KEY_F1, KEY_HOME, KEY_LEFT, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_RIGHT, KEY_SHIFT,
    KEY_SPACE, KEY_TAB, KEY_UP,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub kind: u32, // KBD_EVT_DOWN / UP / CHAR
    pub code: u32, // KEY_* or a code point
    pub mods: u32, // KBD_MOD_* held at the time
}

// Shared between the device and the minifb callback: the callback only ever
// runs inside window.update() on the emulator thread, so an Rc<RefCell> is
// enough -- no locking.
pub type HostKeyQueue = Rc<RefCell<VecDeque<KeyEvent>>>;

pub struct KeyboardDevice {
    events: VecDeque<KeyEvent>,
    pub dropped: u64,
}

impl KeyboardDevice {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(KBD_EVENT_QUEUE_DEPTH),
            dropped: 0,
        }
    }

    // Queue one event. Returns true (raise an IRQ). On overflow the oldest
    // entry is dropped so the newest keystrokes are the ones that survive.
    pub fn push(&mut self, ev: KeyEvent) -> bool {
        if self.events.len() == KBD_EVENT_QUEUE_DEPTH {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(ev);
        true
    }

    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    pub fn head(&self) -> KeyEvent {
        self.events.front().copied().unwrap_or(KeyEvent {
            kind: 0,
            code: 0,
            mods: 0,
        })
    }

    pub fn pop(&mut self) {
        self.events.pop_front();
    }
}

// minifb -> KEY_* code. Letters and digits map to their lowercase ASCII so a
// guest shortcut table can be written as plain characters; the host-translated
// character (with shift applied) arrives separately as a CHAR event.
pub fn key_code(key: Key) -> Option<u32> {
    use Key::*;
    let code = match key {
        A => b'a' as u32,
        B => b'b' as u32,
        C => b'c' as u32,
        D => b'd' as u32,
        E => b'e' as u32,
        F => b'f' as u32,
        G => b'g' as u32,
        H => b'h' as u32,
        I => b'i' as u32,
        J => b'j' as u32,
        K => b'k' as u32,
        L => b'l' as u32,
        M => b'm' as u32,
        N => b'n' as u32,
        O => b'o' as u32,
        P => b'p' as u32,
        Q => b'q' as u32,
        R => b'r' as u32,
        S => b's' as u32,
        T => b't' as u32,
        U => b'u' as u32,
        V => b'v' as u32,
        W => b'w' as u32,
        X => b'x' as u32,
        Y => b'y' as u32,
        Z => b'z' as u32,
        Key0 | NumPad0 => b'0' as u32,
        Key1 | NumPad1 => b'1' as u32,
        Key2 | NumPad2 => b'2' as u32,
        Key3 | NumPad3 => b'3' as u32,
        Key4 | NumPad4 => b'4' as u32,
        Key5 | NumPad5 => b'5' as u32,
        Key6 | NumPad6 => b'6' as u32,
        Key7 | NumPad7 => b'7' as u32,
        Key8 | NumPad8 => b'8' as u32,
        Key9 | NumPad9 => b'9' as u32,
        Space => KEY_SPACE,
        Enter | NumPadEnter => KEY_ENTER,
        Backspace => KEY_BACKSPACE,
        Tab => KEY_TAB,
        Escape => KEY_ESCAPE,
        Delete => KEY_DELETE,
        Left => KEY_LEFT,
        Right => KEY_RIGHT,
        Up => KEY_UP,
        Down => KEY_DOWN,
        Home => KEY_HOME,
        End => KEY_END,
        PageUp => KEY_PAGE_UP,
        PageDown => KEY_PAGE_DOWN,
        LeftShift | RightShift => KEY_SHIFT,
        LeftCtrl | RightCtrl => KEY_CTRL,
        LeftAlt | RightAlt => KEY_ALT,
        F1 => KEY_F1,
        F2 => KEY_F1 + 1,
        F3 => KEY_F1 + 2,
        F4 => KEY_F1 + 3,
        F5 => KEY_F1 + 4,
        F6 => KEY_F1 + 5,
        F7 => KEY_F1 + 6,
        F8 => KEY_F1 + 7,
        F9 => KEY_F1 + 8,
        F10 => KEY_F1 + 9,
        F11 => KEY_F1 + 10,
        F12 => KEY_F1 + 11,
        Minus | NumPadMinus => b'-' as u32,
        Equal => b'=' as u32,
        LeftBracket => b'[' as u32,
        RightBracket => b']' as u32,
        Backslash => b'\\' as u32,
        Semicolon => b';' as u32,
        Apostrophe => b'\'' as u32,
        Comma => b',' as u32,
        Period | NumPadDot => b'.' as u32,
        Slash | NumPadSlash => b'/' as u32,
        Backquote => b'`' as u32,
        NumPadAsterisk => b'*' as u32,
        NumPadPlus => b'+' as u32,
        _ => return None,
    };
    Some(code)
}

// The minifb callback. Tracks modifier state itself from the key events it
// sees, so every queued event carries the modifiers held at that moment.
pub struct HostKeyCallback {
    queue: HostKeyQueue,
    mods: u32,
}

impl HostKeyCallback {
    pub fn new(queue: HostKeyQueue) -> Self {
        Self { queue, mods: 0 }
    }
}

impl InputCallback for HostKeyCallback {
    fn add_char(&mut self, uni_char: u32) {
        // Control characters (Enter, Backspace, Escape, ...) already arrive
        // as DOWN events with a KEY_* code; only visible text goes out as CHAR.
        if uni_char < 0x20 || uni_char == 0x7F {
            return;
        }
        self.queue.borrow_mut().push_back(KeyEvent {
            kind: KBD_EVT_CHAR,
            code: uni_char,
            mods: self.mods,
        });
    }

    fn set_key_state(&mut self, key: Key, state: bool) {
        let Some(code) = key_code(key) else {
            return;
        };
        let modifier = match code {
            KEY_SHIFT => KBD_MOD_SHIFT,
            KEY_CTRL => KBD_MOD_CTRL,
            KEY_ALT => KBD_MOD_ALT,
            _ => 0,
        };
        if state {
            self.mods |= modifier;
        } else {
            self.mods &= !modifier;
        }
        self.queue.borrow_mut().push_back(KeyEvent {
            kind: if state { KBD_EVT_DOWN } else { KBD_EVT_UP },
            code,
            mods: self.mods,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_keystroke_orders_down_char_up() {
        let q: HostKeyQueue = Rc::new(RefCell::new(VecDeque::new()));
        let mut cb = HostKeyCallback::new(q.clone());
        cb.set_key_state(Key::LeftShift, true);
        cb.set_key_state(Key::A, true);
        cb.add_char('A' as u32);
        cb.set_key_state(Key::A, false);
        cb.set_key_state(Key::LeftShift, false);
        let events: Vec<KeyEvent> = q.borrow().iter().copied().collect();
        assert_eq!(events.len(), 5);
        assert_eq!(events[1], KeyEvent { kind: KBD_EVT_DOWN, code: b'a' as u32, mods: KBD_MOD_SHIFT });
        assert_eq!(events[2], KeyEvent { kind: KBD_EVT_CHAR, code: b'A' as u32, mods: KBD_MOD_SHIFT });
        assert_eq!(events[3].kind, KBD_EVT_UP);
        assert_eq!(events[4].mods, 0, "shift released clears the modifier");
    }

    #[test]
    fn control_characters_are_not_text() {
        let q: HostKeyQueue = Rc::new(RefCell::new(VecDeque::new()));
        let mut cb = HostKeyCallback::new(q.clone());
        cb.add_char('\r' as u32);
        cb.add_char(8);
        cb.add_char('x' as u32);
        assert_eq!(q.borrow().len(), 1);
    }

    #[test]
    fn overflow_keeps_newest() {
        let mut dev = KeyboardDevice::new();
        for i in 0..(KBD_EVENT_QUEUE_DEPTH as u32 + 3) {
            dev.push(KeyEvent { kind: KBD_EVT_CHAR, code: i, mods: 0 });
        }
        assert_eq!(dev.event_count(), KBD_EVENT_QUEUE_DEPTH as u32);
        assert_eq!(dev.head().code, 3);
        assert_eq!(dev.dropped, 3);
    }
}
