use std::io::Write;
use std::time::{Duration, Instant};

use crate::instruction::{Opcode, decode_instruction, mnemonic};

use super::{DebugOptions, Machine};

impl Machine {
    // MYOS-004: advance the machine for up to max_instructions (or until it
    // halts, or a wall-clock safety cap elapses, whichever comes first) and
    // return. Used by control_stdio.rs to give the guest a chance to react to
    // an injected input event between automation commands.
    //
    // Shares run_loop/step with normal execution, without CLI pause messages.
    // control_stdio owns stdin, so this entry point does not start serial input.
    pub fn run_frame_budget(&mut self, max_instructions: u64) -> Result<(), String> {
        self.run_loop(
            DebugOptions::default(),
            Some(max_instructions),
            Some(Instant::now() + Duration::from_millis(50)),
        )
    }

    // Exactly one CPU transition; both frontends use the same IRQ, exception,
    // breakpoint, trace and retirement logic. Device polling belongs outside.
    pub(super) fn step(&mut self, options: DebugOptions) -> Result<StepOutcome, String> {
        if self.cpu.halted {
            return Ok(StepOutcome::Halted);
        }
        self.try_dispatch_irq()?;
        if self.cpu.waiting_for_interrupt {
            return Ok(StepOutcome::Idle);
        }
        let pc = self.cpu.program_counter;
        if options.break_addr == Some(pc) {
            return Ok(StepOutcome::Breakpoint);
        }
        self.note_pc();
        let instruction = match self.fetch_instruction() {
            Ok(inst) => inst,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return Ok(StepOutcome::Faulted);
            }
        };
        let inst = decode_instruction(instruction);
        if self.execution.verbose || options.trace {
            let line = format!(
                "PC: 0x{:08X}, Instruction: 0x{:08X}, {} r1=0x{:X} r2=0x{:X} imm=0x{:X}",
                pc,
                instruction,
                mnemonic(inst.opcode),
                inst.reg1,
                inst.reg2,
                inst.imm
            );
            if self.execution.control_stdio {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
            if let Some(log) = self.execution.trace_log.as_mut() {
                writeln!(log, "{line}").map_err(|e| e.to_string())?;
            }
        }
        if Opcode::try_from(inst.opcode).is_err() && self.is_user_mode() {
            self.interrupts
                .raise_trap(crate::constants::IRQ_CAUSE_ILLEGAL_INSTRUCTION);
            return Ok(StepOutcome::Faulted);
        }
        // The mode and page directory the instruction runs under, before it can
        // change them.
        let space = self.execution.profiler.is_some().then(|| self.profile_space());
        self.cpu.program_counter = pc.wrapping_add(4);
        match self.execute_instruction(instruction) {
            Ok(()) => {}
            Err(super::cpu::CpuError::Fault(fault)) => {
                self.cpu.program_counter = pc;
                self.raise_mmu_fault(fault);
                return Ok(StepOutcome::Faulted);
            }
            Err(super::cpu::CpuError::Host(message)) => return Err(self.describe_error(message)),
        }
        self.cpu.instrs_retired = self.cpu.instrs_retired.wrapping_add(1);
        if let Some(space) = space {
            self.profile_retired(pc, space, inst.opcode);
        }
        Ok(StepOutcome::Retired)
    }

    fn profile_retired(&mut self, pc: u32, space: u32, opcode: u8) {
        let next_pc = self.cpu.program_counter;
        let link = self.cpu.link_register;
        let sp = self.cpu.stack_pointer;
        let Some(profiler) = self.execution.profiler.as_mut() else {
            return;
        };
        let new_space = profiler.record_instruction(pc, space, opcode);
        if opcode == 0x1b {
            profiler.record_call(next_pc, space, link, sp);
        } else if next_pc != pc.wrapping_add(4) {
            profiler.record_jump(next_pc, sp);
        }
        if new_space {
            let base = pc & !0xFFF;
            let sample = self.profile_text_sample(base);
            if let Some(profiler) = self.execution.profiler.as_mut() {
                profiler.set_text_sample(space, base, sample);
            }
        }
    }

    fn note_pc(&mut self) {
        self.execution.note_pc(self.cpu.program_counter);
    }

    // Append the recent-PC ring (when enabled) to a machine error message.
    pub fn describe_error(&self, e: String) -> String {
        let Some(ring) = self.execution.recent_pcs() else {
            return e;
        };
        let pcs: Vec<_> = ring.into_iter().map(|pc| format!("{pc:x}")).collect();
        format!("{} | recent pcs: {}", e, pcs.join(" "))
    }

    pub fn set_instruction_pointer(&mut self, address: u32) {
        self.cpu.program_counter = address;
    }

    pub fn load_program(&mut self, program: &[u32], mut start_address: u32) {
        for instruction in program {
            self.bus_write_physical(start_address, *instruction);
            start_address = start_address.wrapping_add(4);
        }
    }

    #[allow(dead_code)]
    pub fn execute(&mut self) -> Result<(), String> {
        self.execute_with_debug(DebugOptions::default())
    }

    pub fn execute_with_debug(&mut self, options: DebugOptions) -> Result<(), String> {
        self.start_serial_input();
        self.run_loop(options, None, None)
    }

    fn run_loop(
        &mut self,
        options: DebugOptions,
        budget: Option<u64>,
        deadline: Option<Instant>,
    ) -> Result<(), String> {
        let mut attempts = 0u64;
        let mut retired = 0u64;
        while !self.cpu.halted {
            if budget.is_some_and(|limit| attempts >= limit) {
                break;
            }
            if options.step_count.is_some_and(|limit| attempts >= limit) {
                println!("[STEP] paused after {retired} instruction(s)");
                self.print_registers();
                break;
            }
            // Persistent cadence: repeated small automation budgets must not
            // indefinitely postpone servicing a pending device operation.
            if self.execution.service_ticks == 0 || self.cpu.waiting_for_interrupt {
                if deadline.is_some_and(|end| Instant::now() >= end) {
                    break;
                }
                if crate::signals::requested() {
                    self.cpu.halted = true;
                    break;
                }
                self.poll_devices();
                self.poll_input();
                if !self.maybe_refresh_display(false) {
                    self.cpu.halted = true;
                    break;
                }
            }
            self.execution.service_ticks = (self.execution.service_ticks + 1) % 1000;
            match self.step(options)? {
                StepOutcome::Idle => {
                    if options.step_count.is_some() {
                        println!("[STEP] CPU idle (WFI) after {retired} instruction(s)");
                        self.print_registers();
                        break;
                    }
                    let cap = Duration::from_millis(4);
                    let mut wait = self
                        .devices
                        .timer
                        .time_until_next()
                        .map_or(cap, |t| t.min(cap));
                    if let Some(end) = deadline {
                        wait = wait.min(end.saturating_duration_since(Instant::now()));
                    }
                    if !wait.is_zero() {
                        let start = Instant::now();
                        std::thread::sleep(wait);
                        if let Some(stats) = self.io_stats.as_mut() {
                            let ns = start.elapsed().as_nanos();
                            stats.wfi_sleep_ns += ns;
                            stats.wfi_sleeps += 1;
                            stats.wfi_sleep_max_ns = stats.wfi_sleep_max_ns.max(ns);
                        }
                    }
                }
                StepOutcome::Breakpoint => {
                    println!("[BREAK] hit 0x{:08X}", self.cpu.program_counter);
                    self.print_registers();
                    break;
                }
                StepOutcome::Halted => break,
                StepOutcome::Faulted => {
                    attempts += 1;
                }
                StepOutcome::Retired => {
                    attempts += 1;
                    retired += 1;
                }
            }
        }
        if self.cpu.halted {
            self.maybe_refresh_display(true);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StepOutcome {
    Retired,
    Faulted,
    Idle,
    Breakpoint,
    Halted,
}
