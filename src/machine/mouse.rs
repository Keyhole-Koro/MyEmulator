pub struct MouseDevice {
    pub x: u32,
    pub y: u32,
    pub buttons: u32,
}

impl MouseDevice {
    pub fn new() -> Self {
        Self { x: 0, y: 0, buttons: 0 }
    }

    pub fn update(&mut self, x: u32, y: u32, buttons: u32) -> bool {
        if x != self.x || y != self.y || buttons != self.buttons {
            self.x = x;
            self.y = y;
            self.buttons = buttons;
            true
        } else {
            false
        }
    }
}
