use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::constants::{
    SSD_BLOCK_COUNT, SSD_BLOCK_SIZE, SSD_CMD_READ, SSD_CMD_WRITE, SSD_DISK_SIZE,
    SSD_STATUS_DONE, SSD_STATUS_ERROR, SSD_STATUS_IDLE,
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
// are synchronous: writing the command register triggers an immediate copy
// between the disk image and emulator RAM (a stand-in for DMA).
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
}

impl SsdDevice {
    pub fn disabled() -> Self {
        Self {
            file: None,
            enabled: false,
            block: 0,
            buf_addr: 0,
            status: SSD_STATUS_IDLE,
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

    // A write to the command register. The device decides what transfer is
    // needed and returns it for the bus to carry out, or None (status already
    // set to error) if the command cannot run.
    fn begin_command(&mut self, cmd: u32) -> Option<SsdOp> {
        if !self.enabled {
            self.status = SSD_STATUS_ERROR;
            return None;
        }
        match cmd {
            SSD_CMD_READ => Some(SsdOp::Read {
                block: self.block,
                buf_addr: self.buf_addr,
            }),
            SSD_CMD_WRITE => Some(SsdOp::Write {
                block: self.block,
                buf_addr: self.buf_addr,
            }),
            _ => {
                self.status = SSD_STATUS_ERROR;
                None
            }
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
    // Service an SSD command-register write. The device decides what transfer to
    // perform and owns the status register; the bus only moves the RAM half of
    // the block (scatter for reads, gather for writes), since only it can reach
    // RAM. This is the synchronous stand-in for DMA. Kept as a Machine method so
    // it can touch both the device and RAM, but housed here to keep all SSD
    // behaviour in one file.
    pub(super) fn service_ssd_dma(&mut self, cmd: u32) {
        match self.ssd.begin_command(cmd) {
            Some(SsdOp::Read { block, buf_addr }) => {
                if let Some(data) = self.ssd.finish_read(block) {
                    for (i, byte) in data.iter().enumerate() {
                        self.bus_write_byte(buf_addr + i as u32, *byte);
                    }
                }
            }
            Some(SsdOp::Write { block, buf_addr }) => {
                let mut data = [0u8; SSD_BLOCK_SIZE];
                for (i, byte) in data.iter_mut().enumerate() {
                    *byte = self.bus_read_byte(buf_addr + i as u32);
                }
                self.ssd.finish_write(block, &data);
            }
            None => {}
        }
    }
}
