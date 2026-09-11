pub const RAM_START: u32 = 0x0000_0000;
pub const RAM_SIZE: u32 = 0x2000_0000; // 512 MB
pub const RAM_END_EXCLUSIVE: u32 = RAM_START.wrapping_add(RAM_SIZE);

pub const VRAM_BASE: u32 = 0x3000_0000;
include!(concat!(env!("OUT_DIR"), "/display_constants.rs"));
pub const VRAM_SIZE: u32 = (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4) as u32;
pub const VRAM_END_EXCLUSIVE: u32 = VRAM_BASE + VRAM_SIZE;

pub const ROM_START: u32 = 0x2000_0000;
pub const ROM_SIZE: u32 = 0x0400_0000; // 64 MB
pub const ROM_END_EXCLUSIVE: u32 = ROM_START + ROM_SIZE;

// Display refresh rate of the emulated display controller (~60 Hz).
pub const DISPLAY_REFRESH_HZ: u64 = 60;

pub fn is_vram_address(address: u32) -> bool {
    (VRAM_BASE..VRAM_END_EXCLUSIVE).contains(&address)
}

pub fn is_rom_address(address: u32) -> bool {
    (ROM_START..ROM_END_EXCLUSIVE).contains(&address)
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

// 2D accelerator. DEST/WIDTH/HEIGHT/STRIDE describe the destination rectangle
// (STRIDE in pixels); COLOR is 0x00RRGGBB for opaque commands and 0xAARRGGBB
// where alpha is involved. SRC/SRC_STRIDE describe the source for the copy and
// mask commands (see DMA2D_CMD_* for units). Writing CMD runs the operation.
pub const DMA2D_DEST_ADDR: u32 = IO_BASE + 0x20;
pub const DMA2D_COLOR_ADDR: u32 = IO_BASE + 0x24;
pub const DMA2D_WIDTH_ADDR: u32 = IO_BASE + 0x28;
pub const DMA2D_HEIGHT_ADDR: u32 = IO_BASE + 0x2C;
pub const DMA2D_STRIDE_ADDR: u32 = IO_BASE + 0x30;
pub const DMA2D_CMD_ADDR: u32 = IO_BASE + 0x34; // W: DMA2D_CMD_*

pub const DISPLAY_SWAP_ADDR: u32 = IO_BASE + 0x38;

pub const DMA2D_SRC_ADDR: u32 = IO_BASE + 0x90; // source: RAM or VRAM address
pub const DMA2D_SRC_STRIDE_ADDR: u32 = IO_BASE + 0x94; // COPY*: pixels, MASK_A8: bytes
pub const DMA2D_COLOR2_ADDR: u32 = IO_BASE + 0x98; // GRADIENT_*: end colour
pub const DMA2D_RADIUS_ADDR: u32 = IO_BASE + 0x9C; // ROUND_RECT*/SHADOW: corner radius (px)
pub const DMA2D_SPREAD_ADDR: u32 = IO_BASE + 0xB8; // ROUND_RECT_OUTLINE: thickness; SHADOW: blur size

// Scissor rectangle, applied to every command: pixels outside
// [X0, X1) x [Y0, Y1) are not written. X1 <= X0 (the reset state) disables it.
// Lets the guest clip a rounded rectangle without changing its shape.
pub const DMA2D_CLIP_X0_ADDR: u32 = IO_BASE + 0xC0;
pub const DMA2D_CLIP_Y0_ADDR: u32 = IO_BASE + 0xC4;
pub const DMA2D_CLIP_X1_ADDR: u32 = IO_BASE + 0xC8;
pub const DMA2D_CLIP_Y1_ADDR: u32 = IO_BASE + 0xCC;

pub const DMA2D_CMD_FILL: u32 = 1; // dest = COLOR
pub const DMA2D_CMD_BLEND_FILL: u32 = 2; // dest = lerp(dest, COLOR.rgb, COLOR.a)
pub const DMA2D_CMD_COPY: u32 = 3; // dest = src (0x00RRGGBB, alpha ignored)
pub const DMA2D_CMD_COPY_BLEND: u32 = 4; // dest = lerp(dest, src.rgb, src.a)
pub const DMA2D_CMD_MASK_A8: u32 = 5; // dest = lerp(dest, COLOR.rgb, src byte)
pub const DMA2D_CMD_GRADIENT_V: u32 = 6; // rows fade COLOR -> COLOR2
pub const DMA2D_CMD_GRADIENT_H: u32 = 7; // columns fade COLOR -> COLOR2
pub const DMA2D_CMD_ROUND_RECT: u32 = 8; // anti-aliased rounded rect, COLOR with alpha (0 = opaque)
pub const DMA2D_CMD_ROUND_RECT_OUTLINE: u32 = 9; // as above, ring of SPREAD px inside the rect
pub const DMA2D_CMD_SHADOW: u32 = 10; // soft shadow of the rounded rect, fading over SPREAD px outside it

// Keyboard. Every host key press/release and every translated character lands
// in a FIFO in the order it happened; STATUS is the queued count, TYPE/CODE/
// MODS read the head event, writing POP consumes it.
pub const KBD_EVT_STATUS_ADDR: u32 = IO_BASE + 0xA0;
pub const KBD_EVT_TYPE_ADDR: u32 = IO_BASE + 0xA4;
pub const KBD_EVT_CODE_ADDR: u32 = IO_BASE + 0xA8;
pub const KBD_EVT_MODS_ADDR: u32 = IO_BASE + 0xAC;
pub const KBD_EVT_POP_ADDR: u32 = IO_BASE + 0xB0;
pub const KBD_EVENT_QUEUE_DEPTH: usize = 64;
pub const KBD_EVT_DOWN: u32 = 1;
pub const KBD_EVT_UP: u32 = 2;
pub const KBD_EVT_CHAR: u32 = 3; // CODE = unicode code point (ASCII in practice)
pub const KBD_MOD_SHIFT: u32 = 1 << 0;
pub const KBD_MOD_CTRL: u32 = 1 << 1;
pub const KBD_MOD_ALT: u32 = 1 << 2;

// Key codes for KBD_EVT_DOWN/UP. Printable keys use their lowercase ASCII;
// everything else is a code >= 0x100 so the guest can tell them apart.
pub const KEY_BACKSPACE: u32 = 8;
pub const KEY_TAB: u32 = 9;
pub const KEY_ENTER: u32 = 13;
pub const KEY_ESCAPE: u32 = 27;
pub const KEY_SPACE: u32 = 32;
pub const KEY_DELETE: u32 = 127;
pub const KEY_LEFT: u32 = 0x101;
pub const KEY_RIGHT: u32 = 0x102;
pub const KEY_UP: u32 = 0x103;
pub const KEY_DOWN: u32 = 0x104;
pub const KEY_HOME: u32 = 0x105;
pub const KEY_END: u32 = 0x106;
pub const KEY_PAGE_UP: u32 = 0x107;
pub const KEY_PAGE_DOWN: u32 = 0x108;
pub const KEY_SHIFT: u32 = 0x110;
pub const KEY_CTRL: u32 = 0x111;
pub const KEY_ALT: u32 = 0x112;
pub const KEY_F1: u32 = 0x121; // F1..F12 = 0x121..0x12C

// Wheel steps carried by the head mouse event (signed; positive = away from
// the user, i.e. scroll up).
pub const MOUSE_EVT_WHEEL_ADDR: u32 = IO_BASE + 0xB4;

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
pub const MOUSE_BUTTON_RIGHT: u32 = 0x2;
pub const MOUSE_BUTTON_MIDDLE: u32 = 0x4;
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
pub const IRQ_CAUSE_KEYBOARD: u32 = 1 << 7;

// MMU & Virtual Memory MMIO registers
pub const MMU_CTRL_ADDR: u32 = IO_BASE + 0x100; // R/W: Bit 0 = Paging Enable
pub const MMU_PDBR_ADDR: u32 = IO_BASE + 0x104; // R/W: Physical Page Directory Base Register
pub const MMU_FAULT_ADDR: u32 = IO_BASE + 0x108; // R: Virtual address causing page fault
pub const MMU_FAULT_STATUS_ADDR: u32 = IO_BASE + 0x10C; // R: 0=Read, 1=Write, 2=Exec, 3=Priv
pub const KERNEL_SP_ADDR: u32 = IO_BASE + 0x110; // R/W: Kernel Stack Pointer for user traps

// Automation bridge. This is deliberately separate from the serial device:
// control-mode DOM inspection must never depend on a shell prompt or share a
// byte stream with kernel logs. The host writes one command code, the kernel
// polls it and streams a response back through TX, then writes DONE.
pub const AUTOMATION_RX_ADDR: u32 = IO_BASE + 0x120; // R: command code, consumes it
pub const AUTOMATION_STATUS_ADDR: u32 = IO_BASE + 0x124; // bit0=request, bit1=response done
pub const AUTOMATION_TX_ADDR: u32 = IO_BASE + 0x128; // W: response byte
pub const AUTOMATION_DONE_ADDR: u32 = IO_BASE + 0x12C; // W nonzero: response complete
pub const AUTOMATION_STATUS_REQUEST: u32 = 1;
pub const AUTOMATION_STATUS_DONE: u32 = 2;

// Status register bits
pub const SR_IE: u32 = 1 << 0; // 0b0000_0001
pub const SR_CARRY: u32 = 1 << 1; // 0b0000_0010
pub const SR_ZERO: u32 = 1 << 2; // 0b0000_0100
pub const SR_SIGN: u32 = 1 << 3; // 0b0000_1000
pub const SR_OVERFLOW: u32 = 1 << 4; // 0b0001_0000
pub const SR_USER: u32 = 1 << 5; // 0b0010_0000: 0 = Kernel Mode, 1 = User Mode

// Page Table Entry (PTE) bits
pub const PTE_VALID: u32 = 1 << 0; // V: Valid / Present
pub const PTE_WRITABLE: u32 = 1 << 1; // W: Writable
pub const PTE_EXEC: u32 = 1 << 2; // X: Executable
pub const PTE_USER: u32 = 1 << 3; // U: User accessible (0 = Kernel only)
pub const PTE_ACCESSED: u32 = 1 << 4; // A: Accessed
pub const PTE_DIRTY: u32 = 1 << 5; // D: Dirty
pub const PAGE_SIZE: u32 = 4096;

pub fn is_ram_address(address: u32) -> bool {
    address < RAM_END_EXCLUSIVE
}

pub fn is_io_address(address: u32) -> bool {
    (IO_BASE..=IO_END_INCLUSIVE).contains(&address)
}
