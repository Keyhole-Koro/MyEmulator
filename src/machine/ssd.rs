use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::constants::{
    IRQ_CAUSE_SSD, SSD_BLOCK_COUNT, SSD_BLOCK_SIZE, SSD_CMD_READ, SSD_CMD_WRITE, SSD_DISK_SIZE,
    SSD_STATUS_BUSY, SSD_STATUS_DONE, SSD_STATUS_ERROR, SSD_STATUS_IDLE,
};

use super::Machine;

// Immutable command operands captured at CMD write, including physical address.
#[derive(Clone, Copy)]
enum SsdOp {
    // Read `block` from disk; the returned bytes go to RAM at `buf_addr`.
    Read { block: u32, buf_addr: u32 },
    // Gather SSD_BLOCK_SIZE bytes from RAM at `buf_addr`, then hand them back
    // via finish_write() to be written to `block`.
    Write { block: u32, buf_addr: u32 },
}

// Emulated SSD block device backed by a host disk-image file. Block transfers
// are asynchronous like real DMA: writing the command register latches the
// transfer and flips STATUS to BUSY; the machine completes it at the next
// device poll (service_ssd), sets DONE/ERROR, and raises the completion IRQ.
//
// The disk image is the single source of truth: reads and writes seek straight
// into the host file rather than mirroring it in RAM. This keeps emulator memory
// flat regardless of disk size, and the OS stores the file sparsely so an
// all-zero 1 GB image costs almost nothing on disk.
pub struct SsdDevice {
    file: Option<File>,
    enabled: bool,
    // Latched register state. The command handler reads block/buf_addr to know
    // what to transfer; status reflects the outcome of the last command.
    block: u32,
    buf_addr: u32,
    status: u32,
    // Command latched by a CMD-register write, still awaiting completion.
    // Some(cmd) exactly while status == BUSY.
    pending_cmd: Option<SsdOp>,
}

impl SsdDevice {
    pub fn disabled() -> Self {
        Self {
            file: None,
            enabled: false,
            block: 0,
            buf_addr: 0,
            status: SSD_STATUS_IDLE,
            pending_cmd: None,
        }
    }

    // Open (or create) the host disk image, sized to the fixed disk capacity.
    // A new or short file is extended with set_len; the host keeps the unwritten
    // remainder sparse, so this does not allocate the full size up front.
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // Preserve existing disk-image contents; only set_len extends a
            // short image to the emulated capacity.
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("failed to open disk image {}: {}", path.display(), e))?;

        if file.metadata().map_err(|e| e.to_string())?.len() > SSD_DISK_SIZE as u64 {
            return Err("disk image exceeds device capacity".to_string());
        }
        file.set_len(SSD_DISK_SIZE as u64)
            .map_err(|e| format!("failed to size disk image: {}", e))?;

        Ok(Self {
            file: Some(file),
            enabled: true,
            block: 0,
            buf_addr: 0,
            status: SSD_STATUS_IDLE,
            pending_cmd: None,
        })
    }

    // Register writes: BLOCK and ADDR just latch their operands.
    pub fn set_block(&mut self, value: u32) {
        self.block = value;
    }

    pub fn set_addr(&mut self, value: u32) {
        self.buf_addr = value;
    }

    // Status register read. A disabled device always reports an error.
    pub fn status(&self) -> u32 {
        if self.enabled {
            self.status
        } else {
            SSD_STATUS_ERROR
        }
    }

    // A write to the command register. Validates and latches the command and
    // flips status to BUSY; the transfer itself runs asynchronously when the
    // machine next services the device. An invalid command or disabled device
    // reports an error immediately (nothing gets latched).
    fn begin_command(&mut self, cmd: u32) {
        if !self.enabled {
            self.status = SSD_STATUS_ERROR;
            return;
        }
        // A busy device keeps its original command and BUSY status.
        if self.pending_cmd.is_some() {
            return;
        }
        match cmd {
            SSD_CMD_READ | SSD_CMD_WRITE => {
                self.status = SSD_STATUS_BUSY;
                self.pending_cmd = Some(if cmd == SSD_CMD_READ {
                    SsdOp::Read {
                        block: self.block,
                        buf_addr: self.buf_addr,
                    }
                } else {
                    SsdOp::Write {
                        block: self.block,
                        buf_addr: self.buf_addr,
                    }
                });
            }
            _ => {
                self.status = SSD_STATUS_ERROR;
            }
        }
    }

    // Take the captured command; later register writes cannot change it.
    fn take_pending(&mut self) -> Option<SsdOp> {
        self.pending_cmd.take()
    }

    // Complete the disk half of a read into an owned DMA buffer.
    fn finish_read(&mut self, block: u32) -> Option<[u8; SSD_BLOCK_SIZE]> {
        match self.read_block(block) {
            Some(data) => {
                self.status = SSD_STATUS_DONE;
                Some(data)
            }
            None => {
                self.status = SSD_STATUS_ERROR;
                None
            }
        }
    }

    // Complete a write from an already validated physical RAM slice.
    fn finish_write(&mut self, block: u32, data: &[u8; SSD_BLOCK_SIZE]) {
        self.status = if self.write_block(block, data) {
            SSD_STATUS_DONE
        } else {
            SSD_STATUS_ERROR
        };
    }

    // Byte offset of a block, or None if the block number is out of range.
    fn block_offset(block: u32) -> Option<u64> {
        if (block as usize) >= SSD_BLOCK_COUNT {
            return None;
        }
        Some(block as u64 * SSD_BLOCK_SIZE as u64)
    }

    // Read one block from the disk image into a caller-owned buffer. Returns
    // None on a bad block number or any I/O error.
    fn read_block(&mut self, block: u32) -> Option<[u8; SSD_BLOCK_SIZE]> {
        let offset = Self::block_offset(block)?;
        let file = self.file.as_mut()?;
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return None;
        }
        let mut buf = [0u8; SSD_BLOCK_SIZE];
        if file.read_exact(&mut buf).is_err() {
            return None;
        }
        Some(buf)
    }

    // Write one block into the disk image and flush it to the host file.
    fn write_block(&mut self, block: u32, data: &[u8; SSD_BLOCK_SIZE]) -> bool {
        let Some(offset) = Self::block_offset(block) else {
            return false;
        };
        let Some(file) = self.file.as_mut() else {
            return false;
        };
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return false;
        }
        if file.write_all(data).is_err() {
            return false;
        }
        file.flush().is_ok()
    }
}

impl SsdDevice {
    // A DMA transfer is entirely physical and independent of the current CPU
    // address space/mode. Reject its entire range before disk or RAM mutation.
    fn service(&mut self, ram: &mut super::ram::Ram) -> bool {
        let Some(op) = self.take_pending() else {
            return false;
        };
        let (block, addr) = match op {
            SsdOp::Read { block, buf_addr } | SsdOp::Write { block, buf_addr } => (block, buf_addr),
        };
        if Self::block_offset(block).is_none() || ram.range(addr, SSD_BLOCK_SIZE).is_none() {
            self.status = SSD_STATUS_ERROR;
            return true;
        }
        match op {
            SsdOp::Read { .. } => {
                if let Some(data) = self.finish_read(block) {
                    assert!(ram.write(addr, &data), "prevalidated DMA range");
                }
            }
            SsdOp::Write { .. } => {
                let data: &[u8; SSD_BLOCK_SIZE] =
                    ram.read(addr, SSD_BLOCK_SIZE).unwrap().try_into().unwrap();
                self.finish_write(block, data);
            }
        }
        true
    }
}

impl Machine {
    pub(super) fn service_ssd_dma(&mut self, cmd: u32) {
        self.ssd.begin_command(cmd);
    }

    pub(super) fn service_ssd(&mut self) {
        if self.ssd.service(&mut self.ram) {
            self.irq_cause |= IRQ_CAUSE_SSD;
            self.pending_irq = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Machine;
    use crate::constants::{
        IRQ_CAUSE_SSD, SSD_ADDR_ADDR, SSD_BLOCK_ADDR, SSD_CMD_ADDR, SSD_CMD_READ, SSD_CMD_WRITE,
        SSD_STATUS_ADDR, SSD_STATUS_BUSY, SSD_STATUS_DONE, SSD_STATUS_ERROR,
    };

    fn machine_with_disk(name: &str) -> (Machine, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "myemu-ssd-test-{}-{}.img",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut m = Machine::new(false, true);
        m.load_disk(path.clone()).unwrap();
        (m, path)
    }

    #[test]
    fn transfer_is_busy_until_polled_then_raises_completion_irq() {
        let (mut m, path) = machine_with_disk("roundtrip");

        // Write a pattern from RAM to block 3.
        m.bus_write_physical(0x1000, 0xDEAD_BEEF);
        m.bus_write_physical(SSD_BLOCK_ADDR, 3);
        m.bus_write_physical(SSD_ADDR_ADDR, 0x1000);
        m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_WRITE);
        assert_eq!(
            m.bus_read_physical(SSD_STATUS_ADDR),
            SSD_STATUS_BUSY,
            "busy until the device is polled"
        );
        assert!(!m.pending_irq, "no IRQ before the transfer completes");
        m.poll_devices();
        assert_eq!(m.bus_read_physical(SSD_STATUS_ADDR), SSD_STATUS_DONE);
        assert_ne!(
            m.irq_cause & IRQ_CAUSE_SSD,
            0,
            "completion raises the SSD cause"
        );
        assert!(m.pending_irq);

        // Read it back into a different RAM buffer.
        m.irq_cause = 0;
        m.pending_irq = false;
        m.bus_write_physical(SSD_BLOCK_ADDR, 3);
        m.bus_write_physical(SSD_ADDR_ADDR, 0x2000);
        m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_READ);
        assert_eq!(m.bus_read_physical(SSD_STATUS_ADDR), SSD_STATUS_BUSY);
        m.poll_devices();
        assert_eq!(m.bus_read_physical(SSD_STATUS_ADDR), SSD_STATUS_DONE);
        assert_eq!(
            m.bus_read_physical(0x2000),
            0xDEAD_BEEF,
            "block round-trips through the disk"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_command_errors_without_irq() {
        let (mut m, path) = machine_with_disk("badcmd");
        m.bus_write_physical(SSD_CMD_ADDR, 0x99);
        assert_eq!(m.bus_read_physical(SSD_STATUS_ADDR), SSD_STATUS_ERROR);
        m.poll_devices();
        assert!(!m.pending_irq, "nothing was latched, so nothing completes");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_dma_ranges_do_not_touch_ram_disk_or_mmio() {
        use crate::constants::{IRQ_CAUSE_ADDR, RAM_END_EXCLUSIVE, VRAM_BASE};
        let (mut m, path) = machine_with_disk("dma-bounds");
        m.bus_write_physical(0, 0xaabb_ccdd);
        m.bus_write_physical(RAM_END_EXCLUSIVE - 4, 0x1122_3344);
        for addr in [
            RAM_END_EXCLUSIVE - 4,
            VRAM_BASE,
            IRQ_CAUSE_ADDR,
            u32::MAX - 1,
        ] {
            for cmd in [SSD_CMD_READ, SSD_CMD_WRITE] {
                m.bus_write_physical(SSD_ADDR_ADDR, addr);
                m.bus_write_physical(SSD_CMD_ADDR, cmd);
                m.service_ssd();
                assert_eq!(m.ssd.status(), SSD_STATUS_ERROR);
                assert_eq!(m.bus_read_physical(0), 0xaabb_ccdd);
                assert_eq!(m.bus_read_physical(RAM_END_EXCLUSIVE - 4), 0x1122_3344);
                assert_eq!(m.display.vram[0], 0);
                assert_eq!(m.irq_cause, IRQ_CAUSE_SSD);
                assert_eq!(m.ssd.read_block(0).unwrap()[..4], [0; 4]);
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pending_dma_keeps_its_original_operands_and_ignores_busy_commands() {
        let (mut m, path) = machine_with_disk("latched-command");
        m.bus_write_physical(0x1000, 0xaabb_ccdd);
        m.bus_write_physical(0x2000, 0x1122_3344);
        m.bus_write_physical(SSD_BLOCK_ADDR, 1);
        m.bus_write_physical(SSD_ADDR_ADDR, 0x1000);
        m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_WRITE);
        m.bus_write_physical(SSD_BLOCK_ADDR, 2);
        m.bus_write_physical(SSD_ADDR_ADDR, 0x2000);
        m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_READ);
        m.bus_write_physical(SSD_CMD_ADDR, 0xff);
        assert_eq!(m.ssd.status(), SSD_STATUS_BUSY);
        m.service_ssd();
        assert_eq!(m.ssd.status(), SSD_STATUS_DONE);
        assert_eq!(
            m.ssd.read_block(1).unwrap()[..4],
            0xaabb_ccddu32.to_be_bytes()
        );
        assert_eq!(m.ssd.read_block(2).unwrap()[..4], [0; 4]);
        assert_eq!(m.bus_read_physical(0x2000), 0x1122_3344);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn short_execution_budgets_still_service_devices() {
        let (mut m, path) = machine_with_disk("small-budgets");
        m.set_timer_interval(0);
        // mov r0,r0 at enough addresses to cover a polling interval.
        for i in 0..1001 {
            m.bus_write_physical(i * 4, 0x01 << 26);
        }
        m.run_frame_budget(1).unwrap();
        m.bus_write_physical(SSD_ADDR_ADDR, 0x10000);
        m.bus_write_physical(SSD_CMD_ADDR, SSD_CMD_READ);
        for _ in 0..1000 {
            m.run_frame_budget(1).unwrap();
        }
        assert_eq!(m.ssd.status(), SSD_STATUS_DONE);
        std::fs::remove_file(path).unwrap();
    }
}
