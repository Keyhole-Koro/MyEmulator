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
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
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

#[cfg(test)]
mod tests {
    use super::Ram;

    #[test]
    fn range_rejects_zero_length() {
        let ram = Ram::new(16);
        assert_eq!(ram.range(0, 0), None);
        assert_eq!(ram.read(0, 0), None);
    }

    #[test]
    fn range_reaches_exactly_the_last_byte() {
        let ram = Ram::new(16);
        assert_eq!(ram.range(12, 4), Some(12..16));
        assert_eq!(ram.range(0, 16), Some(0..16));
        assert_eq!(ram.range(15, 1), Some(15..16));
    }

    #[test]
    fn range_rejects_one_byte_past_the_end() {
        let ram = Ram::new(16);
        assert_eq!(ram.range(13, 4), None);
        assert_eq!(ram.range(0, 17), None);
        assert_eq!(ram.range(16, 1), None);
    }

    #[test]
    fn range_rejects_an_end_that_overflows_u32() {
        let ram = Ram::new(16);
        assert_eq!(ram.range(u32::MAX, 1), None);
        assert_eq!(ram.range(u32::MAX - 1, 4), None);
        assert_eq!(ram.range(0, u32::MAX as usize + 1), None);
    }

    #[test]
    fn accesses_out_of_range_change_nothing() {
        let mut ram = Ram::new(16);
        ram.write_word(12, 0x0102_0304);
        assert_eq!(ram.as_slice()[12..], [1, 2, 3, 4]);
        assert_eq!(ram.read_word(12), 0x0102_0304);
        // A word straddling the end is neither written nor read.
        assert!(!ram.write(14, &[9, 9, 9, 9]));
        ram.write_word(14, 0xFFFF_FFFF);
        assert_eq!(ram.as_slice()[12..], [1, 2, 3, 4]);
        assert_eq!(ram.read_word(14), 0);
        ram.write_byte(16, 7);
        assert_eq!(ram.read_byte(16), 0);
        assert_eq!(ram.as_slice().len(), 16);
    }
}
