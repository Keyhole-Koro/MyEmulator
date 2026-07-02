pub const RAM_START: u32 = 0x0000_0000;
pub const RAM_SIZE: u32 = 0x2000_0000; // 512 MB
pub const RAM_END_EXCLUSIVE: u32 = RAM_START.wrapping_add(RAM_SIZE);

pub const VRAM_BASE: u32 = 0x3000_0000;
pub const DISPLAY_WIDTH: usize = 320;
pub const DISPLAY_HEIGHT: usize = 240;
pub const VRAM_SIZE: u32 = (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4) as u32;
pub const VRAM_END_EXCLUSIVE: u32 = VRAM_BASE + VRAM_SIZE;

// Display refresh rate of the emulated display controller (~60 Hz).
pub const DISPLAY_REFRESH_HZ: u64 = 60;

pub fn is_vram_address(address: u32) -> bool {
    address >= VRAM_BASE && address < VRAM_END_EXCLUSIVE
}

pub const IO_BASE: u32 = 0x2400_0000;
pub const IO_END_INCLUSIVE: u32 = 0x2400_00FF;

pub const SERIAL_TX_ADDR: u32 = IO_BASE;
pub const SERIAL_RX_ADDR: u32 = IO_BASE + 0x04; // receiver buffer (read consumes a byte)
pub const SERIAL_LSR_ADDR: u32 = IO_BASE + 0x05;
pub const SERIAL_LSR_THRE: u32 = 0x20; // transmit holding register empty
pub const SERIAL_LSR_DR: u32 = 0x01; // data ready (a received byte is waiting)

// SSD block device registers. Writing SSD_CMD triggers a synchronous block
// transfer between the disk image and RAM using the latched BLOCK and ADDR.
pub const SSD_CMD_ADDR: u32 = IO_BASE + 0x10; // W: 1=READ, 2=WRITE
pub const SSD_BLOCK_ADDR: u32 = IO_BASE + 0x14; // W: block number (0-indexed)
pub const SSD_ADDR_ADDR: u32 = IO_BASE + 0x18; // W: RAM buffer address
pub const SSD_STATUS_ADDR: u32 = IO_BASE + 0x1C; // R: 0=idle, 2=done, 0xFF=error
pub const SSD_CMD_READ: u32 = 1;
pub const SSD_CMD_WRITE: u32 = 2;
pub const SSD_STATUS_IDLE: u32 = 0;
pub const SSD_STATUS_DONE: u32 = 2;
pub const SSD_STATUS_ERROR: u32 = 0xFF;
pub const SSD_BLOCK_SIZE: usize = 65536;
pub const SSD_BLOCK_COUNT: usize = 16384;
pub const SSD_DISK_SIZE: usize = SSD_BLOCK_SIZE * SSD_BLOCK_COUNT; // 1 GB

// Single fixed IRQ vector slot inside the I/O region. The kernel stores the
// address of its interrupt handler here; on a timer interrupt the CPU reads it
// to find where to jump. Living in I/O space keeps it isolated from RAM/heap.
pub const IRQ_VECTOR_ADDR: u32 = IO_BASE + 0x80;

// Status register interrupt-enable bit.
pub const SR_IE: u32 = 0x1;

pub fn is_ram_address(address: u32) -> bool {
    address >= RAM_START && address < RAM_END_EXCLUSIVE
}

pub fn is_io_address(address: u32) -> bool {
    address >= IO_BASE && address <= IO_END_INCLUSIVE
}
