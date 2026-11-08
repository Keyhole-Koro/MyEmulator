use super::*;
use crate::constants::*;
use crate::machine::mmu::AccessType;

fn paged_machine() -> Machine {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    m.bus_write_physical(0x1000, 0x2000 | PTE_VALID | PTE_USER | PTE_WRITABLE);
    m.memory.mmu.set_pdbr(0x1000);
    m.memory.mmu.set_enabled(true);
    m
}

fn map(m: &mut Machine, va: u32, pa: u32, flags: u32) {
    m.bus_write_physical(0x2000 + ((va >> 12) & 0x3ff) * 4, pa | flags);
    m.memory.mmu.flush_tlb();
}

#[test]
fn user_mov_to_sr_must_not_enter_kernel_mode() {
    let mut m = paged_machine();
    m.restore_status_register(SR_USER | SR_IE);
    m.cpu.registers[0] = 0;
    assert!(matches!(
        m.execute_instruction((0x01 << 26) | (0x0b << 21)),
        Err(cpu::CpuError::Fault(
            mmu::MmuFault::PrivilegeViolation { .. }
        ))
    )); // mov sr, r0
    assert!(m.is_user_mode(), "mov sr,r0 cleared SR_USER without a trap");
}

#[test]
fn unknown_user_opcode_raises_illegal_instruction_trap() {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    m.restore_status_register(SR_USER);
    m.bus_write_physical(0, 0);
    assert_eq!(
        m.step(DebugOptions::default()).unwrap(),
        run_loop::StepOutcome::Faulted
    );
    assert_eq!(
        m.interrupts.pending_trap,
        Some(IRQ_CAUSE_ILLEGAL_INSTRUCTION)
    );
    assert_eq!(m.cpu.program_counter, 0);
}

#[test]
fn atomic_swap_replaces_memory_and_returns_previous_word() {
    let mut m = Machine::new(false, true);
    m.cpu.registers[0] = 0x1122_3344;
    m.cpu.registers[1] = 0x1000;
    m.bus_write_physical(0x1000, 0xaabb_ccdd);
    m.execute_instruction((0x22 << 26) | (0 << 21) | (1 << 16))
        .unwrap();
    assert_eq!(m.cpu.registers[0], 0xaabb_ccdd);
    assert_eq!(m.bus_read_physical(0x1000), 0x1122_3344);
}

#[test]
fn compare_exchange_writes_only_when_expected_matches() {
    let mut m = Machine::new(false, true);
    m.cpu.registers[0] = 7;
    m.cpu.registers[1] = 9;
    m.cpu.registers[2] = 0x1000;
    m.bus_write_physical(0x1000, 7);
    let cas = (0x23 << 26) | (0 << 21) | (1 << 16) | (2 << 11);
    m.execute_instruction(cas).unwrap();
    assert_eq!(m.cpu.registers[0], 7);
    assert_eq!(m.bus_read_physical(0x1000), 9);

    m.cpu.registers[0] = 4;
    m.cpu.registers[1] = 11;
    m.execute_instruction(cas).unwrap();
    assert_eq!(m.cpu.registers[0], 9);
    assert_eq!(m.bus_read_physical(0x1000), 9);
}

#[test]
fn compare_exchange_requires_write_permission_even_on_mismatch() {
    let mut m = paged_machine();
    map(&mut m, 0x4000, 0x8000, PTE_VALID | PTE_USER);
    m.restore_status_register(SR_USER);
    m.cpu.registers[0] = 1;
    m.cpu.registers[1] = 2;
    m.cpu.registers[2] = 0x4000;
    m.bus_write_physical(0x8000, 7);
    let cas = (0x23 << 26) | (0 << 21) | (1 << 16) | (2 << 11);
    assert!(matches!(
        m.execute_instruction(cas),
        Err(cpu::CpuError::Fault(mmu::MmuFault::PageFault { status: 1, .. }))
    ));
    assert_eq!(m.cpu.registers[0], 1);
    assert_eq!(m.bus_read_physical(0x8000), 7);
}

#[test]
fn warm_tlb_must_enforce_directory_user_permission() {
    let mut m = paged_machine();
    m.bus_write_physical(0x1000, 0x2000 | PTE_VALID); // kernel-only directory
    map(&mut m, 0x4000, 0x8000, PTE_VALID | PTE_USER | PTE_WRITABLE);
    assert!(
        m.memory
            .mmu
            .translate(m.memory.ram.as_mut_slice(), 0x4000, AccessType::Read, true)
            .is_err()
    );
    assert!(
        m.memory
            .mmu
            .translate(m.memory.ram.as_mut_slice(), 0x4000, AccessType::Read, false)
            .is_ok()
    );
    assert!(
        m.memory
            .mmu
            .translate(m.memory.ram.as_mut_slice(), 0x4000, AccessType::Read, true)
            .is_err(),
        "kernel TLB fill bypassed the PDE's user restriction"
    );
}

#[test]
fn cross_page_load_must_translate_second_page() {
    let mut m = paged_machine();
    map(&mut m, 0x4000, 0x8000, PTE_VALID | PTE_WRITABLE);
    map(&mut m, 0x5000, 0xa000, PTE_VALID | PTE_WRITABLE);
    m.bus_write_physical(0x8ffc, 0x0000_aabb);
    m.bus_write_physical(0x9000, 0xdead_0000);
    m.bus_write_physical(0xa000, 0xccdd_0000);
    assert_eq!(
        m.bus_load(0x4ffe).unwrap(),
        0xaabb_ccdd,
        "LD read physically adjacent bytes instead of the next virtual page"
    );
}

#[test]
fn faulting_load_must_preserve_destination_register() {
    let mut m = paged_machine();
    map(&mut m, 0, 0x3000, PTE_VALID | PTE_EXEC);
    m.bus_write_physical(0x3000, (0x03 << 26) | (1 << 16)); // ld r0, r1
    m.cpu.registers[0] = 0x1234;
    m.cpu.registers[1] = 0x4000; // unmapped
    m.run_frame_budget(1).unwrap();
    assert_eq!(m.interrupts.pending_trap, Some(IRQ_CAUSE_PAGE_FAULT));
    assert_eq!(
        m.cpu.registers[0], 0x1234,
        "faulting LD overwrote r0 with zero"
    );
}

#[test]
fn faulting_load_must_save_faulting_pc_for_retry() {
    let mut m = paged_machine();
    map(&mut m, 0, 0x3000, PTE_VALID | PTE_EXEC);
    map(&mut m, 0x5000, 0x6000, PTE_VALID | PTE_WRITABLE);
    m.cpu.stack_pointer = 0x5800;
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.bus_write_physical(0x3000, (0x03 << 26) | (1 << 16));
    m.cpu.registers[1] = 0x4000;
    m.run_frame_budget(1).unwrap();
    m.try_dispatch_irq().unwrap();
    assert_eq!(
        m.bus_load(m.cpu.stack_pointer + 4).unwrap(),
        0,
        "saved PC skips the faulting LD"
    );
}

#[test]
fn ssd_dma_read_and_write_must_use_same_address_space() {
    let mut m = paged_machine();
    let path =
        std::env::temp_dir().join(format!("myemu-dma-regression-{}.img", std::process::id()));
    m.load_disk(path.clone()).unwrap();
    for n in 0..16 {
        map(
            &mut m,
            0x10000 + n * 4096,
            0x30000 + n * 4096,
            PTE_VALID | PTE_WRITABLE,
        );
    }
    m.bus_write_physical(0x10000, 0xdead_beef);
    m.bus_write_physical(0x30000, 0xaabb_ccdd);
    m.bus_write_physical(SSD_ADDR_ADDR, 0x10000);
    m.bus_write_physical(SSD_BLOCK_ADDR, 0);
    m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_WRITE);
    m.service_ssd();
    m.bus_write_physical(0x10000, 0);
    m.bus_write_physical(0x30000, 0);
    m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_READ);
    m.service_ssd();
    assert_eq!(m.bus_read_physical(SSD_STATUS_ADDR), SSD_STATUS_DONE);
    assert_eq!(
        m.bus_read_physical(0x10000),
        0xdead_beef,
        "SSD read wrote through MMU even though SSD write read physical RAM"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn irq_handler_entry_breakpoint_must_stop_before_handler() {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.bus_write_physical(0x100, 0x3f << 26); // halt
    m.set_interrupt_enable(true);
    m.ingest_serial_bytes(b"x");
    m.execute_with_debug(DebugOptions {
        break_addr: Some(0x100),
        ..Default::default()
    })
    .unwrap();
    assert!(!m.cpu.halted, "breakpoint skipped: handler's HALT executed");
}

#[test]
fn every_sr_destination_is_checked_before_operand_side_effects() {
    for opcode in [
        0x01, 0x02, 0x18, 0x03, 0x1c, 0x05, 0x19, 0x06, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x15, 0x16,
        0x22, 0x23,
    ] {
        let mut m = Machine::new(false, true);
        m.restore_status_register(SR_USER | SR_IE | SR_CARRY);
        m.cpu.program_counter = 4;
        m.cpu.registers[0] = SERIAL_RX_ADDR;
        m.ingest_serial_bytes(b"x");
        let sp = m.cpu.stack_pointer;
        let sr = m.status_register();
        let result = m.execute_instruction((opcode << 26) | (0x0b << 21));
        assert!(matches!(
            result,
            Err(cpu::CpuError::Fault(mmu::MmuFault::PrivilegeViolation {
                vaddr: 0
            }))
        ));
        assert_eq!(m.status_register(), sr, "opcode {opcode:x}");
        assert_eq!(m.cpu.stack_pointer, sp);
        assert_eq!(m.devices.serial.peek_rx(), u32::from(b'x'));
    }
}

#[test]
fn data_fault_can_be_repaired_and_the_instruction_retried() {
    let mut m = paged_machine();
    map(&mut m, 0, 0x3000, PTE_VALID | PTE_EXEC);
    map(&mut m, 0x5000, 0x6000, PTE_VALID | PTE_WRITABLE);
    m.cpu.stack_pointer = 0x5800;
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.bus_write_physical(0x3000, (0x03 << 26) | (1 << 16)); // ld r0,r1
    m.bus_write_physical(0x3100, 0x20 << 26); // iret
    m.cpu.registers[0] = 0x1234;
    m.cpu.registers[1] = 0x4000;
    assert_eq!(
        m.step(DebugOptions::default()).unwrap(),
        run_loop::StepOutcome::Faulted
    );
    assert_eq!(m.cpu.instrs_retired, 0);
    m.try_dispatch_irq().unwrap();
    assert_eq!(m.memory.mmu.fault_addr, 0x4000);
    map(&mut m, 0x4000, 0x8000, PTE_VALID);
    m.bus_write_physical(0x8000, 0xaabb_ccdd);
    m.bus_write_physical(IRQ_CAUSE_ADDR, IRQ_CAUSE_PAGE_FAULT);
    m.step(DebugOptions::default()).unwrap(); // iret
    assert_eq!(m.cpu.program_counter, 0);
    m.step(DebugOptions::default()).unwrap(); // retry ld
    assert_eq!(m.cpu.registers[0], 0xaabb_ccdd);
    assert_eq!(m.cpu.stack_pointer, 0x5800);
    assert_eq!(m.cpu.program_counter, 4);
}

#[test]
fn syscall_saves_next_pc_and_trap_cause_is_not_a_retrigger() {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    m.cpu.stack_pointer = 0x7000;
    m.memory.mmu.kernel_sp = 0x8000;
    m.restore_status_register(SR_USER); // IE=0 must not mask a syscall
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.bus_write_physical(0, 0x3e << 26);
    m.bus_write_physical(0x100, 0x20 << 26);
    m.step(DebugOptions::default()).unwrap();
    m.try_dispatch_irq().unwrap();
    assert_eq!(m.bus_read_physical(0x7ffc), 4);
    assert_eq!(m.bus_read_physical(0x7ff8), SR_USER);
    assert!(!m.is_user_mode());
    m.interrupts.pending_irq = true; // an unrelated asynchronous event during handler
    m.interrupts.cause |= IRQ_CAUSE_SERIAL;
    m.try_dispatch_irq().unwrap();
    assert_eq!(
        m.cpu.stack_pointer, 0x7ff8,
        "stale syscall cause must not nest"
    );
    m.step(DebugOptions::default()).unwrap();
    assert!(m.is_user_mode());
    assert_eq!(m.cpu.program_counter, 4);
    assert_eq!(m.cpu.stack_pointer, 0x7000);
    assert_eq!(m.memory.mmu.kernel_sp, 0x8000);
}

#[test]
fn cross_page_store_preflights_both_pages_before_writing() {
    let mut m = paged_machine();
    map(&mut m, 0x4000, 0x8000, PTE_VALID | PTE_WRITABLE);
    map(&mut m, 0x5000, 0xa000, PTE_VALID); // read-only
    m.bus_write_physical(0x8ffc, 0x1122_3344);
    m.bus_write_physical(0xa000, 0x5566_7788);
    assert_eq!(
        m.bus_write(0x4ffe, 0xaabb_ccdd),
        Err(mmu::MmuFault::PageFault {
            vaddr: 0x5000,
            status: 1
        })
    );
    assert_eq!(m.bus_read_physical(0x8ffc), 0x1122_3344);
    assert_eq!(m.bus_read_physical(0xa000), 0x5566_7788);
    map(&mut m, 0x5000, 0xa000, PTE_VALID | PTE_WRITABLE);
    m.bus_write(0x4ffe, 0xaabb_ccdd).unwrap();
    assert_eq!(m.bus_read_physical(0x8ffc), 0x1122_aabb);
    assert_eq!(m.bus_read_physical(0xa000), 0xccdd_7788);
}

#[test]
fn faults_in_push_pop_and_iret_preserve_stack_state() {
    for opcode in [0x14, 0x15, 0x20] {
        let mut m = paged_machine();
        map(&mut m, 0x4000, 0x8000, PTE_VALID | PTE_WRITABLE);
        m.cpu.stack_pointer = if opcode == 0x20 { 0x4ffc } else { 0x6000 };
        m.bus_write_physical(0x8ffc, SR_USER);
        let before = m.cpu.clone();
        assert!(matches!(
            m.execute_instruction(opcode << 26),
            Err(cpu::CpuError::Fault(_))
        ));
        assert_eq!(m.cpu.stack_pointer, before.stack_pointer);
        assert_eq!(m.cpu.status_register, before.status_register);
        assert_eq!(m.cpu.program_counter, before.program_counter);
        assert_eq!(m.cpu.registers, before.registers);
    }
}

#[test]
fn interrupt_entry_preflights_the_entire_frame() {
    let mut m = paged_machine();
    map(&mut m, 0x5000, 0x9000, PTE_VALID | PTE_WRITABLE);
    m.bus_write_physical(0x9000, 0xaabb_ccdd);
    m.cpu.stack_pointer = 0x7770;
    m.memory.mmu.kernel_sp = 0x5004; // PC word mapped; SR word unmapped
    m.restore_status_register(SR_USER | SR_IE);
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.ingest_serial_bytes(b"x");
    assert!(m.try_dispatch_irq().is_err());
    assert_eq!(m.bus_read_physical(0x9000), 0xaabb_ccdd);
    assert_eq!(m.cpu.stack_pointer, 0x7770);
    assert_eq!(m.memory.mmu.kernel_sp, 0x5004);
    assert_eq!(m.status_register(), SR_USER | SR_IE);
    assert!(m.interrupts.pending_irq);
}

#[test]
fn mmio_width_alignment_and_ram_end_are_checked() {
    let mut m = Machine::new(false, true);
    m.ingest_serial_bytes(b"xy");
    assert!(m.bus_load(SERIAL_RX_ADDR + 2).is_err());
    assert!(m.bus_write_byte(SSD_CMD_ADDR, 1).is_err());
    assert_eq!(m.bus_load_byte(SERIAL_RX_ADDR).unwrap(), b'x');
    assert_eq!(m.bus_load(SERIAL_RX_ADDR).unwrap(), u32::from(b'y'));
    assert!(m.bus_write(RAM_END_EXCLUSIVE - 2, 0xffff_ffff).is_err());
    assert_eq!(m.bus_read_physical(RAM_END_EXCLUSIVE - 4), 0);
    assert!(m.bus_load(u32::MAX - 1).is_err());
}

#[test]
fn trace_uses_handler_pc_after_irq_dispatch() {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    let path =
        std::env::temp_dir().join(format!("myemu-trace-regression-{}.log", std::process::id()));
    m.set_trace_log(path.clone()).unwrap();
    m.bus_write_physical(IRQ_VECTOR_ADDR, 0x100);
    m.bus_write_physical(0x100, 0x3f << 26);
    m.set_interrupt_enable(true);
    m.ingest_serial_bytes(b"x");
    m.step(DebugOptions {
        trace: true,
        ..Default::default()
    })
    .unwrap();
    let trace = std::fs::read_to_string(&path).unwrap();
    assert!(trace.contains("PC: 0x00000100"));
    assert!(!trace.contains("PC: 0x00000000"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn geometry_registers_are_read_only_and_match_the_framebuffer() {
    let mut m = Machine::new(false, true);
    assert_eq!(
        m.bus_load(DISPLAY_WIDTH_ADDR).unwrap(),
        DISPLAY_WIDTH as u32
    );
    assert_eq!(
        m.bus_load(DISPLAY_HEIGHT_ADDR).unwrap(),
        DISPLAY_HEIGHT as u32
    );
    m.bus_write(DISPLAY_WIDTH_ADDR, 1).unwrap();
    m.bus_write(DISPLAY_HEIGHT_ADDR, 1).unwrap();
    assert_eq!(
        m.bus_load(DISPLAY_WIDTH_ADDR).unwrap() as usize
            * m.bus_load(DISPLAY_HEIGHT_ADDR).unwrap() as usize,
        m.devices.display.vram.len()
    );
}

#[test]
fn check_passes_an_index_below_the_limit() {
    let mut m = Machine::new(false, true);
    m.cpu.registers[3] = 3;
    m.execute_instruction((0x24 << 26) | (3 << 21) | 4).unwrap();
    assert!(!m.cpu.halted);
    assert_eq!(m.cpu.registers[1], 0);
}

#[test]
fn check_halts_with_the_trap_code_at_or_above_the_limit() {
    // Equal to the limit, and a negative index read as unsigned.
    for value in [4u32, 0xFFFF_FFFF] {
        let mut m = Machine::new(false, true);
        m.cpu.program_counter = 0x104; // the CHK at 0x100 has been fetched
        m.cpu.registers[3] = value;
        m.execute_instruction((0x24 << 26) | (3 << 21) | 4).unwrap();
        assert!(m.cpu.halted);
        assert_eq!(m.cpu.registers[1], TRAP_CODE_INDEX);
        assert_eq!(m.cpu.registers[2], 0x100);
    }
}

#[test]
fn check_failure_in_user_mode_is_a_privilege_violation() {
    let mut m = Machine::new(false, true);
    m.restore_status_register(SR_USER);
    m.cpu.program_counter = 0x104;
    m.cpu.registers[3] = 9;
    assert!(matches!(
        m.execute_instruction((0x24 << 26) | (3 << 21) | 4),
        Err(cpu::CpuError::Fault(mmu::MmuFault::PrivilegeViolation { vaddr: 0x100 }))
    ));
    assert!(!m.cpu.halted);
}

#[test]
fn debug_dumps_the_top_of_ram_where_the_stack_starts() {
    let mut m = Machine::new(false, true);
    m.set_timer_interval(0);
    let dir = std::env::temp_dir().join(format!("myemu-debug-regression-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    m.execution.debug_dump_dir = dir.clone();
    m.bus_write_physical(RAM_END_EXCLUSIVE - 0x1000, 0x1122_3344);
    m.bus_write_physical(RAM_END_EXCLUSIVE - 4, 0xaabb_ccdd);
    m.bus_write_physical(0x40, 0x1a << 26); // debug
    m.cpu.program_counter = 0x40;
    assert_eq!(
        m.step(DebugOptions::default()).unwrap(),
        run_loop::StepOutcome::Retired
    );

    // The file is named after the PC, which is past the DEBUG when it runs.
    let dump = std::fs::read_to_string(dir.join("memory_dump68.txt")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let lines: Vec<&str> = dump.lines().collect();
    assert_eq!(lines.len(), 2 + 0x1000 / 4);
    assert_eq!(lines[0], "Memory Dump from 0x1ffff000 to 0x1fffffff");
    assert_eq!(lines[2], "0x1ffff000: 0x11223344");
    assert_eq!(lines[lines.len() - 1], "0x1ffffffc: 0xaabbccdd");
}

#[test]
fn byte_and_word_stores_reach_the_uart_and_automation_tx_alike() {
    let mut m = Machine::new(false, true);
    let path = std::env::temp_dir().join(format!(
        "myemu-serial-regression-{}.txt",
        std::process::id()
    ));
    m.set_serial_log(&path).unwrap();
    m.bus_write_byte(SERIAL_TX_ADDR, b'o').unwrap();
    m.bus_write(SERIAL_TX_ADDR, u32::from(b'k')).unwrap();
    m.bus_write_byte(AUTOMATION_TX_ADDR, b'!').unwrap();
    m.bus_write(AUTOMATION_TX_ADDR, u32::from(b'?')).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"ok");
    std::fs::remove_file(path).unwrap();
    assert_eq!(m.take_automation_response(), b"!?");
    assert_eq!(m.bus_load_byte(SERIAL_TX_ADDR).unwrap(), b'k');
}
