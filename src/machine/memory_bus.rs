use std::io::{self, Write};

use crate::constants::{
    AUTOMATION_DONE_ADDR, AUTOMATION_RX_ADDR, AUTOMATION_STATUS_ADDR, AUTOMATION_STATUS_DONE,
    AUTOMATION_STATUS_REQUEST, AUTOMATION_TX_ADDR, IRQ_CAUSE_ADDR, IRQ_CAUSE_PAGE_FAULT,
    IRQ_CAUSE_PRIVILEGE_VIOLATION, KERNEL_SP_ADDR, MMU_CTRL_ADDR, MMU_FAULT_ADDR,
    MMU_FAULT_STATUS_ADDR, MMU_PDBR_ADDR, SERIAL_LSR_ADDR, SERIAL_RX_ADDR, SERIAL_TX_ADDR,
    SSD_ADDR_ADDR, SSD_BLOCK_ADDR, SSD_CMD_ADDR, SSD_STATUS_ADDR, VRAM_BASE, is_io_address,
    is_ram_address, is_vram_address,
};
use crate::machine::mmu::{AccessType, MmuFault};

use super::Machine;

/// CPU-facing bus contract. Instruction execution uses this narrow surface;
/// translation, access preflight and MMIO dispatch remain behind the bus.
pub(super) trait CpuBus {
    fn load_word(&mut self, address: u32) -> Result<u32, MmuFault>;
    fn store_word(&mut self, address: u32, value: u32) -> Result<(), MmuFault>;
    fn load_byte(&mut self, address: u32) -> Result<u8, MmuFault>;
    fn store_byte(&mut self, address: u32, value: u8) -> Result<(), MmuFault>;
}

impl CpuBus for Machine {
    fn load_word(&mut self, address: u32) -> Result<u32, MmuFault> {
        self.bus_load(address)
    }

    fn store_word(&mut self, address: u32, value: u32) -> Result<(), MmuFault> {
        self.bus_write(address, value)
    }

    fn load_byte(&mut self, address: u32) -> Result<u8, MmuFault> {
        self.bus_load_byte(address)
    }

    fn store_byte(&mut self, address: u32, value: u8) -> Result<(), MmuFault> {
        self.bus_write_byte(address, value)
    }
}

impl Machine {
    pub(super) fn translate_addr(
        &mut self,
        vaddr: u32,
        access: AccessType,
    ) -> Result<u32, MmuFault> {
        let is_user = self.is_user_mode();
        self.memory
            .mmu
            .translate(self.memory.ram.as_mut_slice(), vaddr, access, is_user)
    }

    pub(super) fn raise_mmu_fault(&mut self, fault: MmuFault) {
        self.memory.mmu.record_fault(fault);
        self.interrupts.raise_trap(match fault {
            MmuFault::PageFault { .. } => IRQ_CAUSE_PAGE_FAULT,
            MmuFault::PrivilegeViolation { .. } => IRQ_CAUSE_PRIVILEGE_VIOLATION,
        });
    }

    fn access_fault(address: u32, access: AccessType) -> MmuFault {
        MmuFault::PageFault {
            vaddr: address,
            status: access as u32,
        }
    }

    // Preflight every page and physical region before any data/MMIO side effect.
    // A/D metadata may be updated by the MMU even when a later page faults.
    pub(super) fn prepare_word(
        &mut self,
        address: u32,
        access: AccessType,
    ) -> Result<[u32; 4], MmuFault> {
        let end = address
            .checked_add(3)
            .ok_or(Self::access_fault(address, access))?;
        let first = self.translate_addr(address, access)?;
        let mut addresses = [first; 4];
        for (i, slot) in addresses.iter_mut().enumerate().skip(1) {
            *slot = if (address >> 12) == ((address + i as u32) >> 12) {
                first
                    .checked_add(i as u32)
                    .ok_or(Self::access_fault(address, access))?
            } else {
                self.translate_addr(address + i as u32, access)?
            };
        }
        if is_io_address(first) {
            // UART LSR is the one legacy byte-addressed register also read by LD.
            if access == AccessType::Execute
                || (!first.is_multiple_of(4) && first != SERIAL_LSR_ADDR)
                || !addresses.iter().all(|&p| is_io_address(p))
                || addresses[3] != first + 3
            {
                return Err(Self::access_fault(address, access));
            }
            return Ok(addresses);
        }
        for (i, &p) in addresses.iter().enumerate() {
            let valid = is_ram_address(p)
                || (crate::constants::is_rom_address(p) && access != AccessType::Write)
                || (is_vram_address(p) && access != AccessType::Execute);
            if !valid {
                return Err(Self::access_fault(end - 3 + i as u32, access));
            }
        }
        Ok(addresses)
    }

    pub(super) fn read_prepared_word(&mut self, addresses: [u32; 4]) -> u32 {
        let first = addresses[0];
        if first == SERIAL_RX_ADDR {
            return self.devices.serial.read_rx();
        }
        if first == AUTOMATION_RX_ADDR {
            return self.memory.automation.request.take().unwrap_or(0);
        }
        if is_io_address(first) || (first.is_multiple_of(4) && addresses[3] == first + 3) {
            return self.bus_read_physical(first);
        }
        u32::from_be_bytes(addresses.map(|p| self.bus_read_byte_physical(p)))
    }

    pub(super) fn write_prepared_word(&mut self, addresses: [u32; 4], value: u32) {
        let first = addresses[0];
        if is_io_address(first) || (first.is_multiple_of(4) && addresses[3] == first + 3) {
            self.bus_write_physical(first, value);
        } else {
            for (p, byte) in addresses.into_iter().zip(value.to_be_bytes()) {
                self.bus_write_byte_physical(p, byte);
            }
        }
    }

    pub(super) fn fetch_instruction(&mut self) -> Result<u32, MmuFault> {
        let pc = self.cpu.program_counter;
        if !pc.is_multiple_of(4) {
            return Err(Self::access_fault(pc, AccessType::Execute));
        }
        let addresses = self.prepare_word(pc, AccessType::Execute)?;
        Ok(self.bus_read_physical(addresses[0]))
    }

    // CPU accesses use virtual addresses and propagate faults to step().
    pub(super) fn bus_load(&mut self, address: u32) -> Result<u32, MmuFault> {
        let addresses = self.prepare_word(address, AccessType::Read)?;
        Ok(self.read_prepared_word(addresses))
    }

    pub(super) fn bus_write(&mut self, address: u32, value: u32) -> Result<(), MmuFault> {
        let addresses = self.prepare_word(address, AccessType::Write)?;
        self.write_prepared_word(addresses, value);
        Ok(())
    }

    fn prepare_byte(&mut self, address: u32, access: AccessType) -> Result<u32, MmuFault> {
        let p = self.translate_addr(address, access)?;
        let valid = is_ram_address(p)
            || is_vram_address(p)
            || (crate::constants::is_rom_address(p) && access == AccessType::Read)
            || matches!(
                p,
                SERIAL_TX_ADDR
                    | SERIAL_RX_ADDR
                    | SERIAL_LSR_ADDR
                    | AUTOMATION_RX_ADDR
                    | AUTOMATION_STATUS_ADDR
                    | AUTOMATION_TX_ADDR
                    | AUTOMATION_DONE_ADDR
            );
        if !valid {
            return Err(Self::access_fault(address, access));
        }
        Ok(p)
    }

    pub(super) fn bus_load_byte(&mut self, address: u32) -> Result<u8, MmuFault> {
        let p = self.prepare_byte(address, AccessType::Read)?;
        if p == SERIAL_RX_ADDR {
            return Ok(self.devices.serial.read_rx() as u8);
        }
        if p == AUTOMATION_RX_ADDR {
            return Ok(self.memory.automation.request.take().unwrap_or(0) as u8);
        }
        Ok(self.bus_read_byte_physical(p))
    }

    pub(super) fn bus_write_byte(&mut self, address: u32, value: u8) -> Result<(), MmuFault> {
        let p = self.prepare_byte(address, AccessType::Write)?;
        if is_io_address(p) {
            self.bus_write_physical(p, value as u32);
        } else {
            self.bus_write_byte_physical(p, value);
        }
        Ok(())
    }

    pub(super) fn bus_write_physical(&mut self, address: u32, value: u32) {
        if is_ram_address(address) {
            self.ram_write_word(address, value);
            return;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize / 4;
            if offset < self.devices.display.vram.len() {
                self.devices.display.vram[offset] = value;
            }
            return;
        }

        if is_io_address(address) {
            match address {
                crate::constants::DISPLAY_WIDTH_ADDR | crate::constants::DISPLAY_HEIGHT_ADDR => {
                    return;
                }
                MMU_CTRL_ADDR => {
                    self.memory.mmu.set_enabled(value & 1 != 0);
                    return;
                }
                MMU_PDBR_ADDR => {
                    self.memory.mmu.set_pdbr(value);
                    return;
                }
                KERNEL_SP_ADDR => {
                    self.memory.mmu.kernel_sp = value;
                    return;
                }
                AUTOMATION_TX_ADDR => {
                    self.memory.automation.tx_buf.push((value & 0xFF) as u8);
                    return;
                }
                AUTOMATION_DONE_ADDR => {
                    if value != 0 {
                        self.memory.automation.done = true;
                    }
                    return;
                }
                SSD_BLOCK_ADDR => {
                    self.devices.ssd.set_block(value);
                    return;
                }
                SSD_ADDR_ADDR => {
                    self.devices.ssd.set_addr(value);
                    return;
                }
                SSD_CMD_ADDR => {
                    self.service_ssd_dma(value);
                    return;
                }
                crate::constants::DMA2D_CMD_ADDR => {
                    self.service_dma2d(value);
                    return;
                }
                crate::constants::CURSOR_X_ADDR => {
                    self.devices.display.cursor_x = value;
                    return;
                }
                crate::constants::CURSOR_Y_ADDR => {
                    self.devices.display.cursor_y = value;
                    return;
                }
                crate::constants::CURSOR_CTRL_ADDR => {
                    self.devices.display.cursor_visible = value & 1 != 0;
                    return;
                }
                crate::constants::DISPLAY_SWAP_ADDR => {
                    if value != 0 {
                        self.devices.display.swap();
                        self.note_display_swap();
                    }
                    return;
                }
                IRQ_CAUSE_ADDR => {
                    self.interrupts.acknowledge(value); // W1C acknowledgement.
                    return;
                }
                crate::constants::KBD_EVT_POP_ADDR => {
                    if value != 0 {
                        self.devices.keyboard.pop();
                    }
                    return;
                }
                crate::constants::MOUSE_EVT_POP_ADDR => {
                    if value != 0 {
                        let depth = self.devices.mouse.queue_depth();
                        let waited = self.devices.mouse.pop_event();
                        let dropped = self.devices.mouse.dropped;
                        if let Some(stats) = self.io_stats.as_mut() {
                            stats.evt_queue_depth_sum += depth as u128;
                            stats.evt_queue_samples += 1;
                            if depth > stats.evt_queue_depth_max {
                                stats.evt_queue_depth_max = depth;
                            }
                            stats.evt_dropped = dropped;
                            if let Some(w) = waited {
                                let ns = w.as_nanos();
                                stats.evt_latency_ns += ns;
                                stats.evt_latency_samples += 1;
                                if ns > stats.evt_latency_max_ns {
                                    stats.evt_latency_max_ns = ns;
                                }
                                stats.live_lat_ns += ns;
                                stats.live_samples += 1;
                                if ns > stats.live_lat_max_ns {
                                    stats.live_lat_max_ns = ns;
                                }
                                let start = *stats
                                    .live_window_start
                                    .get_or_insert_with(std::time::Instant::now);
                                if start.elapsed().as_secs_f64() >= 1.0 {
                                    stats.live_window_start = Some(std::time::Instant::now());
                                    stats.live_lat_ns = 0;
                                    stats.live_lat_max_ns = 0;
                                    stats.live_samples = 0;
                                }
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
            self.memory.io.insert(address, value);
            if address == SERIAL_TX_ADDR {
                let ch = (value & 0xFF) as u8;
                if self.execution.control_stdio {
                    eprint!("{}", char::from(ch));
                } else {
                    print!("{}", char::from(ch));
                }
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[ch]);
                    let _ = serial_log.flush();
                }
                if self.execution.control_stdio {
                    let _ = io::stderr().flush();
                } else {
                    let _ = io::stdout().flush();
                }
            }
        }
    }

    pub(super) fn bus_read_physical(&self, address: u32) -> u32 {
        if is_ram_address(address) {
            return self.ram_read_word(address);
        }

        if crate::constants::is_rom_address(address) {
            let offset = (address - crate::constants::ROM_START) as usize;
            if offset + 3 < self.memory.rom.len() {
                let b = &self.memory.rom[offset..offset + 4];
                return u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
            }
            return 0;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize / 4;
            if offset < self.devices.display.vram.len() {
                return self.devices.display.vram[offset];
            }
            return 0;
        }

        if is_io_address(address) {
            if address == MMU_CTRL_ADDR {
                return if self.memory.mmu.enabled { 1 } else { 0 };
            }
            if address == crate::constants::DISPLAY_WIDTH_ADDR {
                return crate::constants::DISPLAY_WIDTH as u32;
            }
            if address == crate::constants::DISPLAY_HEIGHT_ADDR {
                return crate::constants::DISPLAY_HEIGHT as u32;
            }
            if address == MMU_PDBR_ADDR {
                return self.memory.mmu.pdbr;
            }
            if address == MMU_FAULT_ADDR {
                return self.memory.mmu.fault_addr;
            }
            if address == MMU_FAULT_STATUS_ADDR {
                return self.memory.mmu.fault_status;
            }
            if address == KERNEL_SP_ADDR {
                return self.memory.mmu.kernel_sp;
            }

            if address == AUTOMATION_STATUS_ADDR {
                let mut status = 0;
                if self.memory.automation.request.is_some() {
                    status |= AUTOMATION_STATUS_REQUEST;
                }
                if self.memory.automation.done {
                    status |= AUTOMATION_STATUS_DONE;
                }
                return status;
            }
            if address == AUTOMATION_RX_ADDR {
                return self.memory.automation.request.unwrap_or(0);
            }

            if address == SSD_STATUS_ADDR {
                return self.devices.ssd.status();
            }
            if address == IRQ_CAUSE_ADDR {
                return self.interrupts.cause;
            }
            if address == crate::constants::MOUSE_X_ADDR {
                return self.devices.mouse.x;
            }
            if address == crate::constants::MOUSE_Y_ADDR {
                return self.devices.mouse.y;
            }
            if address == crate::constants::MOUSE_BUTTONS_ADDR {
                return self.devices.mouse.buttons;
            }
            if address == crate::constants::MOUSE_EVT_STATUS_ADDR {
                return self.devices.mouse.event_count();
            }
            if address == crate::constants::MOUSE_EVT_X_ADDR {
                return self.devices.mouse.head_event().x;
            }
            if address == crate::constants::MOUSE_EVT_Y_ADDR {
                return self.devices.mouse.head_event().y;
            }
            if address == crate::constants::MOUSE_EVT_BTN_ADDR {
                return self.devices.mouse.head_event().buttons;
            }
            if address == crate::constants::MOUSE_EVT_WHEEL_ADDR {
                return self.devices.mouse.head_event().wheel as u32;
            }
            if address == crate::constants::KBD_EVT_STATUS_ADDR {
                return self.devices.keyboard.event_count();
            }
            if address == crate::constants::KBD_EVT_TYPE_ADDR {
                return self.devices.keyboard.head().kind;
            }
            if address == crate::constants::KBD_EVT_CODE_ADDR {
                return self.devices.keyboard.head().code;
            }
            if address == crate::constants::KBD_EVT_MODS_ADDR {
                return self.devices.keyboard.head().mods;
            }
            if address == SERIAL_LSR_ADDR {
                return self.devices.serial.lsr();
            }
            if address == SERIAL_RX_ADDR {
                return self.devices.serial.peek_rx();
            }
            return *self.memory.io.get(&address).unwrap_or(&0xFFFF_FFFF);
        }

        0xFFFF_FFFF
    }

    pub(super) fn bus_write_byte_physical(&mut self, address: u32, value: u8) {
        if is_ram_address(address) {
            self.memory.ram.write_byte(address, value);
            return;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize;
            let word_index = offset / 4;
            if word_index < self.devices.display.vram.len() {
                let byte_shift = (3 - (offset % 4)) * 8; // big-endian
                let mask = !(0xFF << byte_shift);
                let current = self.devices.display.vram[word_index];
                self.devices.display.vram[word_index] =
                    (current & mask) | ((value as u32) << byte_shift);
            }
            return;
        }

        if is_io_address(address) {
            self.memory.io.insert(address, value as u32);
            if address == SERIAL_TX_ADDR {
                if self.execution.control_stdio {
                    eprint!("{}", char::from(value));
                } else {
                    print!("{}", char::from(value));
                }
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[value]);
                    let _ = serial_log.flush();
                }
                if value == b'\n' {
                    if self.execution.control_stdio {
                        let _ = io::stderr().flush();
                    } else {
                        let _ = io::stdout().flush();
                    }
                }
            }
        }
    }

    pub(super) fn bus_read_byte_physical(&self, address: u32) -> u8 {
        if is_ram_address(address) {
            return self.memory.ram.read_byte(address);
        }

        if crate::constants::is_rom_address(address) {
            let offset = (address - crate::constants::ROM_START) as usize;
            if offset < self.memory.rom.len() {
                return self.memory.rom[offset];
            }
            return 0;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize;
            let word_index = offset / 4;
            if word_index < self.devices.display.vram.len() {
                let byte_shift = (3 - (offset % 4)) * 8; // big-endian
                return ((self.devices.display.vram[word_index] >> byte_shift) & 0xFF) as u8;
            }
            return 0;
        }

        if is_io_address(address) {
            let value = if address == AUTOMATION_STATUS_ADDR {
                let mut status = 0;
                if self.memory.automation.request.is_some() {
                    status |= AUTOMATION_STATUS_REQUEST;
                }
                if self.memory.automation.done {
                    status |= AUTOMATION_STATUS_DONE;
                }
                status
            } else if address == AUTOMATION_RX_ADDR {
                self.memory.automation.request.unwrap_or(0)
            } else if address == SERIAL_LSR_ADDR {
                self.devices.serial.lsr()
            } else {
                *self.memory.io.get(&address).unwrap_or(&0xFFFF_FFFF)
            };
            return (value & 0xFF) as u8;
        }

        0xFF
    }

    pub(super) fn ram_read_word(&self, address: u32) -> u32 {
        self.memory.ram.read_word(address)
    }

    pub(super) fn ram_write_word(&mut self, address: u32, value: u32) {
        self.memory.ram.write_word(address, value);
    }

    pub(super) fn read_stack_memory(&self, address: u32) -> u32 {
        self.bus_read_physical(address)
    }
}
