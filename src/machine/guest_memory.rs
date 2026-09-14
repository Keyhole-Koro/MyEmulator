//! Guest-addressable backing storage and address-translation state.
use std::collections::HashMap;

use crate::constants::{RAM_SIZE, ROM_SIZE};

use super::{mmu::Mmu, ram::Ram};

/// Storage owned by the guest memory bus. Host UI and execution diagnostics do
/// not belong here; this keeps physical-memory/MMIO reasoning local.
pub(super) struct GuestMemory {
    pub ram: Ram,
    pub rom: Vec<u8>,
    pub io: HashMap<u32, u32>,
    pub mmu: Mmu,
    pub automation: AutomationBridge,
}

/// Host-to-guest automation registers. They are MMIO state, not execution
/// state: RX consumes a request and TX is a guest-produced byte stream.
pub(super) struct AutomationBridge {
    pub request: Option<u32>,
    pub tx_buf: Vec<u8>,
    pub done: bool,
}

impl GuestMemory {
    pub fn new() -> Self {
        Self {
            ram: Ram::new(RAM_SIZE as usize),
            rom: vec![0; ROM_SIZE as usize],
            io: HashMap::new(),
            mmu: Mmu::new(),
            automation: AutomationBridge {
                request: None,
                tx_buf: Vec::new(),
                done: false,
            },
        }
    }
}
