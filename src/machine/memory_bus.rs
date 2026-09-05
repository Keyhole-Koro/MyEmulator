use std::io::{self, Write};

use crate::constants::{
    is_io_address, is_ram_address, is_vram_address, IRQ_CAUSE_ADDR, IRQ_CAUSE_PAGE_FAULT,
    IRQ_CAUSE_PRIVILEGE_VIOLATION, KERNEL_SP_ADDR, MMU_CTRL_ADDR, MMU_FAULT_ADDR,
    MMU_FAULT_STATUS_ADDR, MMU_PDBR_ADDR, SERIAL_LSR_ADDR, SERIAL_RX_ADDR, SERIAL_TX_ADDR,
    SSD_ADDR_ADDR, SSD_BLOCK_ADDR, SSD_CMD_ADDR, SSD_STATUS_ADDR, VRAM_BASE,
};
use crate::machine::mmu::{AccessType, MmuFault};

use super::Machine;

impl Machine {
    pub(super) fn translate_addr(
        &mut self,
        vaddr: u32,
        access: AccessType,
    ) -> Result<u32, MmuFault> {
        let is_user = self.is_user_mode();
        self.mmu.translate(&mut self.ram, vaddr, access, is_user)
    }

    pub(super) fn raise_mmu_fault(&mut self, fault: MmuFault) {
        match fault {
            MmuFault::PageFault { .. } => {
                self.irq_cause |= IRQ_CAUSE_PAGE_FAULT;
            }
            MmuFault::PrivilegeViolation { .. } => {
                self.irq_cause |= IRQ_CAUSE_PRIVILEGE_VIOLATION;
            }
        }
        self.pending_irq = true;
    }

    pub(super) fn fetch_instruction(&mut self) -> Result<u32, ()> {
        let pc = self.program_counter;
        let paddr = match self.translate_addr(pc, AccessType::Execute) {
            Ok(p) => p,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return Err(());
            }
        };
        Ok(self.bus_read_physical(paddr))
    }

    // Load used by the LD instructions (Virtual memory -> Physical memory).
    pub(super) fn bus_load(&mut self, address: u32) -> u32 {
        let paddr = match self.translate_addr(address, AccessType::Read) {
            Ok(p) => p,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return 0;
            }
        };

        if paddr == SERIAL_RX_ADDR {
            return self.serial.read_rx();
        }
        self.bus_read_physical(paddr)
    }

    pub(super) fn bus_load_byte(&mut self, address: u32) -> u8 {
        let paddr = match self.translate_addr(address, AccessType::Read) {
            Ok(p) => p,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return 0;
            }
        };

        if paddr == SERIAL_RX_ADDR {
            return self.serial.read_rx() as u8;
        }
        self.bus_read_byte_physical(paddr)
    }

    pub(super) fn bus_write(&mut self, address: u32, value: u32) {
        let paddr = match self.translate_addr(address, AccessType::Write) {
            Ok(p) => p,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return;
            }
        };
        self.bus_write_physical(paddr, value);
    }

    pub(super) fn bus_write_byte(&mut self, address: u32, value: u8) {
        let paddr = match self.translate_addr(address, AccessType::Write) {
            Ok(p) => p,
            Err(fault) => {
                self.raise_mmu_fault(fault);
                return;
            }
        };
        self.bus_write_byte_physical(paddr, value);
    }

    pub(super) fn bus_write_physical(&mut self, address: u32, value: u32) {
        if is_ram_address(address) {
            self.ram_write_word(address, value);
            return;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize / 4;
            if offset < self.vram.len() {
                self.vram[offset] = value;
            }
            return;
        }

        if is_io_address(address) {
            match address {
                MMU_CTRL_ADDR => {
                    self.mmu.set_enabled(value & 1 != 0);
                    return;
                }
                MMU_PDBR_ADDR => {
                    self.mmu.set_pdbr(value);
                    return;
                }
                KERNEL_SP_ADDR => {
                    self.mmu.kernel_sp = value;
                    return;
                }
                SSD_BLOCK_ADDR => {
                    self.ssd.set_block(value);
                    return;
                }
                SSD_ADDR_ADDR => {
                    self.ssd.set_addr(value);
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
                    self.cursor_x = value;
                    return;
                }
                crate::constants::CURSOR_Y_ADDR => {
                    self.cursor_y = value;
                    return;
                }
                crate::constants::CURSOR_CTRL_ADDR => {
                    self.cursor_visible = value & 1 != 0;
                    return;
                }
                crate::constants::DISPLAY_SWAP_ADDR => {
                    if value != 0 {
                        self.maybe_refresh_display(false);
                        self.front.copy_from_slice(&self.vram);
                        self.swapped = true;
                        let retired = self.instrs_retired;
                        if let Some(stats) = self.io_stats.as_mut() {
                            if stats.guest_swaps > 0 {
                                let d = retired.saturating_sub(stats.instrs_at_last_swap);
                                stats.instrs_per_frame_sum += d as u128;
                                if d > stats.instrs_per_frame_max {
                                    stats.instrs_per_frame_max = d;
                                }
                            }
                            stats.instrs_at_last_swap = retired;
                            stats.guest_swaps += 1;
                            if let Some(prev) = stats.last_swap {
                                let gap = prev.elapsed().as_nanos();
                                stats.swap_gap_ns += gap;
                                if gap > stats.swap_gap_max_ns {
                                    stats.swap_gap_max_ns = gap;
                                }
                            }
                            stats.last_swap = Some(std::time::Instant::now());
                        }
                    }
                    return;
                }
                IRQ_CAUSE_ADDR => {
                    self.irq_cause &= !value; // Ack (clear) the bits that are written as 1
                    if self.irq_cause == 0 {
                        self.pending_irq = false;
                    }
                    return;
                }
                crate::constants::MOUSE_EVT_POP_ADDR => {
                    if value != 0 {
                        let depth = self.mouse.queue_depth();
                        let waited = self.mouse.pop_event();
                        let dropped = self.mouse.dropped;
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
            self.io.insert(address, value);
            if address == SERIAL_TX_ADDR {
                let ch = (value & 0xFF) as u8;
                print!("{}", char::from(ch));
                self.serial_tx_buf.push(ch);
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[ch]);
                    let _ = serial_log.flush();
                }
                let _ = io::stdout().flush();
            }
        }
    }

    pub(super) fn bus_read(&self, address: u32) -> u32 {
        self.bus_read_physical(address)
    }

    pub(super) fn bus_read_physical(&self, address: u32) -> u32 {
        if is_ram_address(address) {
            return self.ram_read_word(address);
        }

        if crate::constants::is_rom_address(address) {
            let offset = (address - crate::constants::ROM_START) as usize;
            if offset + 3 < self.rom.len() {
                let b = &self.rom[offset..offset + 4];
                return u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
            }
            return 0;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize / 4;
            if offset < self.vram.len() {
                return self.vram[offset];
            }
            return 0;
        }

        if is_io_address(address) {
            if address == MMU_CTRL_ADDR {
                return if self.mmu.enabled { 1 } else { 0 };
            }
            if address == MMU_PDBR_ADDR {
                return self.mmu.pdbr;
            }
            if address == MMU_FAULT_ADDR {
                return self.mmu.fault_addr;
            }
            if address == MMU_FAULT_STATUS_ADDR {
                return self.mmu.fault_status;
            }
            if address == KERNEL_SP_ADDR {
                return self.mmu.kernel_sp;
            }

            if address == SSD_STATUS_ADDR {
                return self.ssd.status();
            }
            if address == IRQ_CAUSE_ADDR {
                return self.irq_cause;
            }
            if address == crate::constants::MOUSE_X_ADDR {
                return self.mouse.x;
            }
            if address == crate::constants::MOUSE_Y_ADDR {
                return self.mouse.y;
            }
            if address == crate::constants::MOUSE_BUTTONS_ADDR {
                return self.mouse.buttons;
            }
            if address == crate::constants::MOUSE_EVT_STATUS_ADDR {
                return self.mouse.event_count();
            }
            if address == crate::constants::MOUSE_EVT_X_ADDR {
                return self.mouse.head_event().x;
            }
            if address == crate::constants::MOUSE_EVT_Y_ADDR {
                return self.mouse.head_event().y;
            }
            if address == crate::constants::MOUSE_EVT_BTN_ADDR {
                return self.mouse.head_event().buttons;
            }
            if address == SERIAL_LSR_ADDR {
                return self.serial.lsr();
            }
            if address == SERIAL_RX_ADDR {
                return self.serial.peek_rx();
            }
            return *self.io.get(&address).unwrap_or(&0xFFFF_FFFF);
        }

        0xFFFF_FFFF
    }

    pub(super) fn bus_write_byte_physical(&mut self, address: u32, value: u8) {
        if is_ram_address(address) {
            self.ram[address as usize] = value;
            return;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize;
            let word_index = offset / 4;
            if word_index < self.vram.len() {
                let byte_shift = (3 - (offset % 4)) * 8; // big-endian
                let mask = !(0xFF << byte_shift);
                let current = self.vram[word_index];
                self.vram[word_index] = (current & mask) | ((value as u32) << byte_shift);
            }
            return;
        }

        if is_io_address(address) {
            self.io.insert(address, value as u32);
            if address == SERIAL_TX_ADDR {
                print!("{}", char::from(value));
                self.serial_tx_buf.push(value);
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[value]);
                    let _ = serial_log.flush();
                }
                if value == b'\n' {
                    let _ = io::stdout().flush();
                }
            }
        }
    }

    pub(super) fn bus_read_byte(&self, address: u32) -> u8 {
        self.bus_read_byte_physical(address)
    }

    pub(super) fn bus_read_byte_physical(&self, address: u32) -> u8 {
        if is_ram_address(address) {
            return self.ram[address as usize];
        }

        if crate::constants::is_rom_address(address) {
            let offset = (address - crate::constants::ROM_START) as usize;
            if offset < self.rom.len() {
                return self.rom[offset];
            }
            return 0;
        }

        if is_vram_address(address) {
            let offset = (address - VRAM_BASE) as usize;
            let word_index = offset / 4;
            if word_index < self.vram.len() {
                let byte_shift = (3 - (offset % 4)) * 8; // big-endian
                return ((self.vram[word_index] >> byte_shift) & 0xFF) as u8;
            }
            return 0;
        }

        if is_io_address(address) {
            let value = if address == SERIAL_LSR_ADDR {
                self.serial.lsr()
            } else {
                *self.io.get(&address).unwrap_or(&0xFFFF_FFFF)
            };
            return (value & 0xFF) as u8;
        }

        0xFF
    }

    pub(super) fn ram_read_word(&self, address: u32) -> u32 {
        let i = address as usize;
        if i + 4 <= self.ram.len() {
            let b = &self.ram[i..i + 4];
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            0
        }
    }

    pub(super) fn ram_write_word(&mut self, address: u32, value: u32) {
        let i = address as usize;
        if i + 4 <= self.ram.len() {
            let bytes = value.to_be_bytes();
            self.ram[i..i + 4].copy_from_slice(&bytes);
        }
    }

    pub(super) fn read_stack_memory(&self, address: u32) -> u32 {
        self.bus_read_physical(address)
    }

    fn service_dma2d(&mut self, cmd: u32) {
        if cmd == 1 {
            let dest = *self
                .io
                .get(&crate::constants::DMA2D_DEST_ADDR)
                .unwrap_or(&0);
            let color = *self
                .io
                .get(&crate::constants::DMA2D_COLOR_ADDR)
                .unwrap_or(&0);
            let width = *self
                .io
                .get(&crate::constants::DMA2D_WIDTH_ADDR)
                .unwrap_or(&0);
            let height = *self
                .io
                .get(&crate::constants::DMA2D_HEIGHT_ADDR)
                .unwrap_or(&0);
            let stride = *self
                .io
                .get(&crate::constants::DMA2D_STRIDE_ADDR)
                .unwrap_or(&(crate::constants::DISPLAY_WIDTH as u32));

            if dest >= crate::constants::VRAM_BASE {
                let start_idx = (dest - crate::constants::VRAM_BASE) as usize / 4;
                let w = width as usize;
                let h = height as usize;
                let s = stride as usize;
                for y in 0..h {
                    let row_start = start_idx + y * s;
                    if row_start + w <= self.vram.len() {
                        self.vram[row_start..row_start + w].fill(color);
                    }
                }
            }
        }
    }
}
