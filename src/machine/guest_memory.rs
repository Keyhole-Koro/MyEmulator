//! Guest-addressable backing storage and address-translation state.
use std::collections::HashMap;

use crate::constants::{AUTOMATION_STATUS_DONE, AUTOMATION_STATUS_REQUEST, RAM_SIZE, ROM_SIZE};

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

impl AutomationBridge {
    /// The STATUS register: a request is waiting, the response is complete.
    pub fn status(&self) -> u32 {
        let mut status = 0;
        if self.request.is_some() {
            status |= AUTOMATION_STATUS_REQUEST;
        }
        if self.done {
            status |= AUTOMATION_STATUS_DONE;
        }
        status
    }
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
