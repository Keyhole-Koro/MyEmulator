pub const RAM_START: u32 = 0x0000_0000;
pub const RAM_SIZE: u32 = 0x2000_0000; // 512 MB
pub const RAM_END_EXCLUSIVE: u32 = RAM_START.wrapping_add(RAM_SIZE);

pub const VRAM_BASE: u32 = 0x3000_0000;
pub const DISPLAY_WIDTH: usize = 1024;
pub const DISPLAY_HEIGHT: usize = 768;
pub const VRAM_SIZE: u32 = (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4) as u32;
pub const VRAM_END_EXCLUSIVE: u32 = VRAM_BASE + VRAM_SIZE;

pub const ROM_START: u32 = 0x2000_0000;
pub const ROM_SIZE: u32 = 0x0400_0000; // 64 MB
pub const ROM_END_EXCLUSIVE: u32 = ROM_START + ROM_SIZE;

// Display refresh rate of the emulated display controller (~60 Hz).
pub const DISPLAY_REFRESH_HZ: u64 = 60;

pub fn is_vram_address(address: u32) -> bool {
    address >= VRAM_BASE && address < VRAM_END_EXCLUSIVE
}

pub fn is_rom_address(address: u32) -> bool {
    address >= ROM_START && address < ROM_END_EXCLUSIVE
}

pub const IO_BASE: u32 = 0x2400_0000;
pub const IO_END_INCLUSIVE: u32 = 0x2400_01FF;

pub const SERIAL_TX_ADDR: u32 = IO_BASE;
pub const SERIAL_RX_ADDR: u32 = IO_BASE + 0x04; // receiver buffer (read consumes a byte)
pub const SERIAL_LSR_ADDR: u32 = IO_BASE + 0x05;
pub const SERIAL_LSR_THRE: u32 = 0x20; // transmit holding register empty
pub const SERIAL_LSR_DR: u32 = 0x01; // data ready (a received byte is waiting)

// SSD block device registers.
pub const SSD_CMD_ADDR: u32 = IO_BASE + 0x10; // W: 1=READ, 2=WRITE
pub const SSD_BLOCK_ADDR: u32 = IO_BASE + 0x14; // W: block number (0-indexed)
pub const SSD_ADDR_ADDR: u32 = IO_BASE + 0x18; // W: RAM buffer address
pub const SSD_STATUS_ADDR: u32 = IO_BASE + 0x1C; // R: 0=idle, 1=busy, 2=done, 0xFF=error
pub const SSD_CMD_READ: u32 = 1;
pub const SSD_CMD_WRITE: u32 = 2;
pub const SSD_STATUS_IDLE: u32 = 0;
pub const SSD_STATUS_BUSY: u32 = 1;
pub const SSD_STATUS_DONE: u32 = 2;
pub const SSD_STATUS_ERROR: u32 = 0xFF;
pub const SSD_BLOCK_SIZE: usize = 65536;
pub const SSD_BLOCK_COUNT: usize = 16384;
pub const SSD_DISK_SIZE: usize = SSD_BLOCK_SIZE * SSD_BLOCK_COUNT; // 1 GB

pub const DMA2D_DEST_ADDR: u32 = IO_BASE + 0x20;
pub const DMA2D_COLOR_ADDR: u32 = IO_BASE + 0x24;
pub const DMA2D_WIDTH_ADDR: u32 = IO_BASE + 0x28;
pub const DMA2D_HEIGHT_ADDR: u32 = IO_BASE + 0x2C;
pub const DMA2D_STRIDE_ADDR: u32 = IO_BASE + 0x30;
pub const DMA2D_CMD_ADDR: u32 = IO_BASE + 0x34; // W: 1 = fill_rect

pub const DISPLAY_SWAP_ADDR: u32 = IO_BASE + 0x38;

pub const CURSOR_X_ADDR: u32 = IO_BASE + 0x60;
pub const CURSOR_Y_ADDR: u32 = IO_BASE + 0x64;
pub const CURSOR_CTRL_ADDR: u32 = IO_BASE + 0x68;

pub const MOUSE_X_ADDR: u32 = IO_BASE + 0x40;
pub const MOUSE_Y_ADDR: u32 = IO_BASE + 0x44;
pub const MOUSE_BUTTONS_ADDR: u32 = IO_BASE + 0x48;

pub const MOUSE_EVT_STATUS_ADDR: u32 = IO_BASE + 0x4C;
pub const MOUSE_EVT_X_ADDR: u32 = IO_BASE + 0x50;
pub const MOUSE_EVT_Y_ADDR: u32 = IO_BASE + 0x54;
pub const MOUSE_EVT_BTN_ADDR: u32 = IO_BASE + 0x58;
pub const MOUSE_EVT_POP_ADDR: u32 = IO_BASE + 0x5C;
pub const MOUSE_EVENT_QUEUE_DEPTH: usize = 64;

pub const MOUSE_BUTTON_LEFT: u32 = 0x1;
pub const MOUSE_POLL_MS: u64 = 2;

pub const IRQ_VECTOR_ADDR: u32 = IO_BASE + 0x80;
pub const IRQ_CAUSE_ADDR: u32 = IO_BASE + 0x84; // R/W
pub const IRQ_CAUSE_TIMER: u32 = 1 << 0;
pub const IRQ_CAUSE_MOUSE: u32 = 1 << 1;
pub const IRQ_CAUSE_SERIAL: u32 = 1 << 2;
pub const IRQ_CAUSE_SSD: u32 = 1 << 3;
pub const IRQ_CAUSE_SYSCALL: u32 = 1 << 4;
pub const IRQ_CAUSE_PAGE_FAULT: u32 = 1 << 5;
pub const IRQ_CAUSE_PRIVILEGE_VIOLATION: u32 = 1 << 6;

// MMU & Virtual Memory MMIO registers
pub const MMU_CTRL_ADDR: u32 = IO_BASE + 0x100;         // R/W: Bit 0 = Paging Enable
pub const MMU_PDBR_ADDR: u32 = IO_BASE + 0x104;         // R/W: Physical Page Directory Base Register
pub const MMU_FAULT_ADDR: u32 = IO_BASE + 0x108;        // R: Virtual address causing page fault
pub const MMU_FAULT_STATUS_ADDR: u32 = IO_BASE + 0x10C; // R: 0=Read, 1=Write, 2=Exec, 3=Priv
pub const KERNEL_SP_ADDR: u32 = IO_BASE + 0x110;        // R/W: Kernel Stack Pointer for user traps

// Status register bits
pub const SR_IE: u32       = 1 << 0; // 0b0000_0001
pub const SR_CARRY: u32    = 1 << 1; // 0b0000_0010
pub const SR_ZERO: u32     = 1 << 2; // 0b0000_0100
pub const SR_SIGN: u32     = 1 << 3; // 0b0000_1000
pub const SR_OVERFLOW: u32 = 1 << 4; // 0b0001_0000
pub const SR_USER: u32     = 1 << 5; // 0b0010_0000: 0 = Kernel Mode, 1 = User Mode

// Page Table Entry (PTE) bits
pub const PTE_VALID: u32    = 1 << 0; // V: Valid / Present
pub const PTE_WRITABLE: u32 = 1 << 1; // W: Writable
pub const PTE_EXEC: u32     = 1 << 2; // X: Executable
pub const PTE_USER: u32     = 1 << 3; // U: User accessible (0 = Kernel only)
pub const PTE_ACCESSED: u32 = 1 << 4; // A: Accessed
pub const PTE_DIRTY: u32    = 1 << 5; // D: Dirty
pub const PAGE_SIZE: u32    = 4096;

pub fn is_ram_address(address: u32) -> bool {
    address >= RAM_START && address < RAM_END_EXCLUSIVE
}

pub fn is_io_address(address: u32) -> bool {
    address >= IO_BASE && address <= IO_END_INCLUSIVE
}
