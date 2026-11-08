//! Host-side execution policy and optional observation tools.
//!
//! This deliberately contains no guest architectural state. CPU registers,
//! MMU state and devices remain deterministic even when tracing or profiling
//! is enabled.
use std::fs::File;
use std::path::PathBuf;

use super::profiler::Profiler;

pub(super) struct ExecutionState {
    pub verbose: bool,
    pub control_stdio: bool,
    pub trace_log: Option<File>,
    pub profiler: Option<Profiler>,
    pub service_ticks: u16,
    // Where DEBUG writes memory_dump<pc>.txt: the working directory, unless a
    // test points it elsewhere.
    pub debug_dump_dir: PathBuf,
    pc_ring: Option<(Vec<u32>, usize)>,
}

impl ExecutionState {
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            control_stdio: false,
            trace_log: None,
            profiler: None,
            service_ticks: 0,
            debug_dump_dir: PathBuf::new(),
            pc_ring: std::env::var("MYEMU_PC_RING").ok().map(|value| {
                let capacity = value.parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(64);
                (vec![0; capacity], 0)
            }),
        }
    }

    pub fn note_pc(&mut self, pc: u32) {
        if let Some((ring, next)) = self.pc_ring.as_mut() {
            ring[*next] = pc;
            *next = (*next + 1) % ring.len();
        }
    }

    pub fn recent_pcs(&self) -> Option<Vec<u32>> {
        let (ring, next) = self.pc_ring.as_ref()?;
        Some(
            (0..ring.len())
                .map(|i| ring[(next + i) % ring.len()])
                .collect(),
        )
    }
}
