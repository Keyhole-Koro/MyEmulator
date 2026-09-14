//! Architectural CPU state and instruction failures. Host errors stop execution;
//! guest faults are delivered to the guest's exception vector by step().
use super::mmu::MmuFault;

#[derive(Clone)]
pub(super) struct Cpu {
    pub registers: [u32; 8],
    pub stack_pointer: u32,
    pub base_pointer: u32,
    pub program_counter: u32,
    pub status_register: u32,
    pub link_register: u32,
    pub halted: bool,
    pub carry_flag: bool,
    pub zero_flag: bool,
    pub sign_flag: bool,
    pub overflow_flag: bool,
    pub interrupt_enable: bool,
    pub waiting_for_interrupt: bool,
    pub instrs_retired: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            registers: [0; 8],
            stack_pointer: crate::constants::RAM_END_EXCLUSIVE,
            base_pointer: 0,
            program_counter: crate::constants::RAM_START,
            status_register: 0,
            link_register: 0,
            halted: false,
            carry_flag: false,
            zero_flag: false,
            sign_flag: false,
            overflow_flag: false,
            interrupt_enable: false,
            waiting_for_interrupt: false,
            instrs_retired: 0,
        }
    }
}

#[derive(Debug)]
pub(super) enum CpuError {
    Fault(MmuFault),
    Host(String),
}

impl From<MmuFault> for CpuError {
    fn from(fault: MmuFault) -> Self {
        Self::Fault(fault)
    }
}

impl From<String> for CpuError {
    fn from(message: String) -> Self {
        Self::Host(message)
    }
}

impl std::fmt::Display for CpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fault(fault) => write!(f, "{fault:?}"),
            Self::Host(message) => f.write_str(message),
        }
    }
}
