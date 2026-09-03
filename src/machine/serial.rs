use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

pub struct SerialDevice {
    rx_queue: VecDeque<u8>,
    rx_recv: Option<Receiver<u8>>,
}

impl SerialDevice {
    pub fn new() -> Self {
        Self {
            rx_queue: VecDeque::new(),
            rx_recv: None,
        }
    }

    pub fn has_rx_recv(&self) -> bool {
        self.rx_recv.is_some()
    }

    pub fn set_rx_recv(&mut self, recv: Receiver<u8>) {
        self.rx_recv = Some(recv);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ingest_bytes(&mut self, bytes: &[u8]) {
        self.rx_queue.extend(bytes.iter().copied());
    }

    pub fn read_rx(&mut self) -> u32 {
        self.rx_queue.pop_front().map(|b| b as u32).unwrap_or(0)
    }

    pub fn peek_rx(&self) -> u32 {
        self.rx_queue.front().map(|b| *b as u32).unwrap_or(0)
    }

    pub fn lsr(&self) -> u32 {
        let mut lsr = crate::constants::SERIAL_LSR_THRE;
        if !self.rx_queue.is_empty() {
            lsr |= crate::constants::SERIAL_LSR_DR;
        }
        lsr
    }

    pub fn service_input(&mut self) -> bool {
        let Some(rx) = &self.rx_recv else {
            return false;
        };
        let mut bytes = Vec::new();
        while let Ok(byte) = rx.try_recv() {
            bytes.push(byte);
        }
        if bytes.is_empty() {
            return false;
        }
        self.rx_queue.extend(bytes.iter().copied());
        true
    }
}
