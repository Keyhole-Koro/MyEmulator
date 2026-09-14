//! Guest-visible devices grouped independently from host presentation.
use super::{
    display::DisplayDevice, keyboard::KeyboardDevice, mouse::MouseDevice, serial::SerialDevice,
    ssd::SsdDevice, timer::TimerDevice,
};

pub(super) struct Devices {
    pub display: DisplayDevice,
    pub serial: SerialDevice,
    pub mouse: MouseDevice,
    pub keyboard: KeyboardDevice,
    pub timer: TimerDevice,
    pub ssd: SsdDevice,
}

impl Devices {
    pub fn new() -> Self {
        Self {
            display: DisplayDevice::new(),
            serial: SerialDevice::new(),
            mouse: MouseDevice::new(),
            keyboard: KeyboardDevice::new(),
            timer: TimerDevice::new(),
            ssd: SsdDevice::disabled(),
        }
    }
}
