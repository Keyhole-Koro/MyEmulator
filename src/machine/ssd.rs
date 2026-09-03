use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::constants::{
    IRQ_CAUSE_SSD, SSD_BLOCK_COUNT, SSD_BLOCK_SIZE, SSD_CMD_READ, SSD_CMD_WRITE, SSD_DISK_SIZE,
    SSD_STATUS_BUSY, SSD_STATUS_DONE, SSD_STATUS_ERROR, SSD_STATUS_IDLE,
};

use super::Machine;

// What the memory bus must do to service a command, decided by the device. The
// bus performs the RAM half of the transfer (scatter/gather) since only it can
// reach RAM; the device performs the disk half and owns the status register.
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
    pending_cmd: Option<u32>,
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
        match cmd {
            SSD_CMD_READ | SSD_CMD_WRITE => {
                self.status = SSD_STATUS_BUSY;
                self.pending_cmd = Some(cmd);
            }
            _ => {
                self.status = SSD_STATUS_ERROR;
            }
        }
    }

    // Take the in-flight command, if any, resolved against the latched
    // block/buffer registers, for the machine to complete.
    fn take_pending(&mut self) -> Option<SsdOp> {
        match self.pending_cmd.take()? {
            SSD_CMD_READ => Some(SsdOp::Read {
                block: self.block,
                buf_addr: self.buf_addr,
            }),
            SSD_CMD_WRITE => Some(SsdOp::Write {
                block: self.block,
                buf_addr: self.buf_addr,
            }),
            _ => None,
        }
    }

    // Complete a read: the bus supplies the block it wants and the device fills
    // the buffer, returning the bytes for the bus to scatter into RAM. Sets the
    // status register from the outcome.
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

    // Complete a write: the bus has gathered the block bytes from RAM and hands
    // them here to be written to disk. Sets the status register from the outcome.
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
        let _ = file.flush();
        true
    }
}

impl Machine {
    // Service an SSD command-register write: the device validates and latches
    // the command and reports BUSY. The transfer completes in service_ssd at
    // the next device poll, like real DMA finishing after the store retires.
    pub(super) fn service_ssd_dma(&mut self, cmd: u32) {
        self.ssd.begin_command(cmd);
    }

    // Complete an in-flight SSD command. The device owns the status register
    // and the disk half of the transfer; the bus moves the RAM half (scatter
    // for reads, gather for writes), since only it can reach RAM. Completion —
    // success or error — raises the SSD IRQ so the guest can stop waiting and
    // read STATUS. Called from poll_devices. Kept as a Machine method so it can
    // touch both the device and RAM, but housed here to keep all SSD behaviour
    // in one file.
    pub(super) fn service_ssd(&mut self) {
        let Some(op) = self.ssd.take_pending() else {
            return;
        };
        match op {
            SsdOp::Read { block, buf_addr } => {
                if let Some(data) = self.ssd.finish_read(block) {
                    for (i, byte) in data.iter().enumerate() {
                        self.bus_write_byte(buf_addr + i as u32, *byte);
                    }
                }
            }
            SsdOp::Write { block, buf_addr } => {
                let mut data = [0u8; SSD_BLOCK_SIZE];
                for (i, byte) in data.iter_mut().enumerate() {
                    *byte = self.bus_read_byte(buf_addr + i as u32);
                }
                self.ssd.finish_write(block, &data);
            }
        }
        self.irq_cause |= IRQ_CAUSE_SSD;
        self.pending_irq = true;
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
        m.bus_write(0x1000, 0xDEAD_BEEF);
        m.bus_write(SSD_BLOCK_ADDR, 3);
        m.bus_write(SSD_ADDR_ADDR, 0x1000);
        m.bus_write(SSD_CMD_ADDR, SSD_CMD_WRITE);
        assert_eq!(
            m.bus_read(SSD_STATUS_ADDR),
            SSD_STATUS_BUSY,
            "busy until the device is polled"
        );
        assert!(!m.pending_irq, "no IRQ before the transfer completes");
        m.poll_devices();
        assert_eq!(m.bus_read(SSD_STATUS_ADDR), SSD_STATUS_DONE);
        assert_ne!(
            m.irq_cause & IRQ_CAUSE_SSD,
            0,
            "completion raises the SSD cause"
        );
        assert!(m.pending_irq);

        // Read it back into a different RAM buffer.
        m.irq_cause = 0;
        m.pending_irq = false;
        m.bus_write(SSD_BLOCK_ADDR, 3);
        m.bus_write(SSD_ADDR_ADDR, 0x2000);
        m.bus_write(SSD_CMD_ADDR, SSD_CMD_READ);
        assert_eq!(m.bus_read(SSD_STATUS_ADDR), SSD_STATUS_BUSY);
        m.poll_devices();
        assert_eq!(m.bus_read(SSD_STATUS_ADDR), SSD_STATUS_DONE);
        assert_eq!(
            m.bus_read(0x2000),
            0xDEAD_BEEF,
            "block round-trips through the disk"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_command_errors_without_irq() {
        let (mut m, path) = machine_with_disk("badcmd");
        m.bus_write(SSD_CMD_ADDR, 0x99);
        assert_eq!(m.bus_read(SSD_STATUS_ADDR), SSD_STATUS_ERROR);
        m.poll_devices();
        assert!(!m.pending_irq, "nothing was latched, so nothing completes");
        let _ = std::fs::remove_file(&path);
    }
}
