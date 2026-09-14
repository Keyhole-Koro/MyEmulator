//! Guest-visible display state; no host window or wall-clock operations.
use crate::constants::VRAM_SIZE;

pub(super) struct DisplayDevice {
    pub vram: Vec<u32>,
    pub front: Vec<u32>,
    pub swapped: bool,
    pub swap_count: u64,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub cursor_visible: bool,
}

impl DisplayDevice {
    pub fn new() -> Self {
        Self {
            vram: vec![0; (VRAM_SIZE / 4) as usize],
            front: vec![0; (VRAM_SIZE / 4) as usize],
            swapped: false,
            swap_count: 0,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: false,
        }
    }

    pub fn swap(&mut self) {
        self.front.copy_from_slice(&self.vram);
        self.swapped = true;
        self.swap_count = self.swap_count.wrapping_add(1);
    }
}
