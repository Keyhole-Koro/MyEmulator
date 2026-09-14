//! Physical RAM only: never translates addresses or dispatches MMIO.
use std::ops::Range;

pub(super) struct Ram {
    bytes: Vec<u8>,
}

impl Ram {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
        }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub fn range(&self, address: u32, len: usize) -> Option<Range<usize>> {
        let len32 = u32::try_from(len).ok()?;
        let end = address.checked_add(len32)? as usize;
        if len == 0 || end > self.bytes.len() {
            return None;
        }
        Some(address as usize..end)
    }

    pub fn read(&self, address: u32, len: usize) -> Option<&[u8]> {
        Some(&self.bytes[self.range(address, len)?])
    }

    pub fn write(&mut self, address: u32, data: &[u8]) -> bool {
        let Some(range) = self.range(address, data.len()) else {
            return false;
        };
        self.bytes[range].copy_from_slice(data);
        true
    }

    pub fn read_byte(&self, address: u32) -> u8 {
        self.bytes.get(address as usize).copied().unwrap_or(0)
    }

    pub fn write_byte(&mut self, address: u32, value: u8) {
        if let Some(byte) = self.bytes.get_mut(address as usize) {
            *byte = value;
        }
    }

    pub fn read_word(&self, address: u32) -> u32 {
        self.read(address, 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .unwrap_or(0)
    }

    pub fn write_word(&mut self, address: u32, value: u32) {
        self.write(address, &value.to_be_bytes());
    }
}
