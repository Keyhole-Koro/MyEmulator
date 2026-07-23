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
pub const IO_END_INCLUSIVE: u32 = 0x2400_00FF;

pub const SERIAL_TX_ADDR: u32 = IO_BASE;
pub const SERIAL_RX_ADDR: u32 = IO_BASE + 0x04; // receiver buffer (read consumes a byte)
pub const SERIAL_LSR_ADDR: u32 = IO_BASE + 0x05;
pub const SERIAL_LSR_THRE: u32 = 0x20; // transmit holding register empty
pub const SERIAL_LSR_DR: u32 = 0x01; // data ready (a received byte is waiting)

// SSD block device registers. Writing SSD_CMD latches an asynchronous block
// transfer between the disk image and RAM using the latched BLOCK and ADDR:
// STATUS reads BUSY until the device completes the DMA (at the next device
// poll), then flips to DONE/ERROR and raises IRQ_CAUSE_SSD.
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

// Display control. Writing 1 to DISPLAY_SWAP copies the back buffer (where all
// VRAM writes land) to the front buffer that the display scans out, so a frame
// is only ever shown once fully drawn (no tearing). Until the first swap the
// back buffer is presented directly, so programs that never swap still render.
pub const DISPLAY_SWAP_ADDR: u32 = IO_BASE + 0x38; // W: 1 = present back buffer

// Mouse device registers. The host pointer is sampled every ~2 ms (independent
// of the 60 Hz display refresh); a change in position or button state raises an
// IRQ (same shared vector as the timer/serial) and is also latched into the
// event FIFO below. The live registers always hold the current state.
pub const MOUSE_X_ADDR: u32 = IO_BASE + 0x40; // R: cursor x in display pixels
pub const MOUSE_Y_ADDR: u32 = IO_BASE + 0x44; // R: cursor y in display pixels
pub const MOUSE_BUTTONS_ADDR: u32 = IO_BASE + 0x48; // R: bit0 = left button down

// Mouse event FIFO. One entry is queued per observed change so the guest sees
// every transition (e.g. a press and release between two reads) instead of
// only the latest state. On overflow the oldest entry is dropped, so the
// newest state is always retained. EVT_X/Y/BTN read the head entry; writing
// EVT_POP consumes it.
pub const MOUSE_EVT_STATUS_ADDR: u32 = IO_BASE + 0x4C; // R: queued event count
pub const MOUSE_EVT_X_ADDR: u32 = IO_BASE + 0x50; // R: head event x
pub const MOUSE_EVT_Y_ADDR: u32 = IO_BASE + 0x54; // R: head event y
pub const MOUSE_EVT_BTN_ADDR: u32 = IO_BASE + 0x58; // R: head event buttons
pub const MOUSE_EVT_POP_ADDR: u32 = IO_BASE + 0x5C; // W: pop the head event
pub const MOUSE_EVENT_QUEUE_DEPTH: usize = 64;

pub const MOUSE_BUTTON_LEFT: u32 = 0x1;

// How often the host pointer is sampled and its events pumped, in
// milliseconds. Much faster than the display refresh so brief clicks between
// frames still land in the event FIFO.
pub const MOUSE_POLL_MS: u64 = 2;

pub const IRQ_VECTOR_ADDR: u32 = IO_BASE + 0x80;

// Interrupt Cause Register. The hardware sets these bits when an event occurs,
// and raises pending_irq. The kernel reads this to dispatch the interrupt,
// then writes to it to clear the bits it handled.
pub const IRQ_CAUSE_ADDR: u32 = IO_BASE + 0x84; // R/W
pub const IRQ_CAUSE_TIMER: u32 = 1 << 0;
pub const IRQ_CAUSE_MOUSE: u32 = 1 << 1;
pub const IRQ_CAUSE_SERIAL: u32 = 1 << 2;
pub const IRQ_CAUSE_SSD: u32 = 1 << 3; // block transfer completed (check STATUS)

// Status register interrupt-enable bit.
pub const SR_IE: u32       = 0b0000_0001;
pub const SR_CARRY: u32    = 0b0000_0010;
pub const SR_ZERO: u32     = 0b0000_0100;
pub const SR_SIGN: u32     = 0b0000_1000;
pub const SR_OVERFLOW: u32 = 0b0001_0000;

pub fn is_ram_address(address: u32) -> bool {
    address >= RAM_START && address < RAM_END_EXCLUSIVE
}

pub fn is_io_address(address: u32) -> bool {
    address >= IO_BASE && address <= IO_END_INCLUSIVE
}
