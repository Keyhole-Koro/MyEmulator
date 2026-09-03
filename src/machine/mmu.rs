use crate::constants::{
    PAGE_SIZE, PTE_ACCESSED, PTE_DIRTY, PTE_EXEC, PTE_USER, PTE_VALID, PTE_WRITABLE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read = 0,
    Write = 1,
    Execute = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmuFault {
    PageFault { vaddr: u32, status: u32 },
    PrivilegeViolation { vaddr: u32 },
}

#[derive(Clone, Copy, Default)]
struct TlbEntry {
    vpn: u32,
    pfn: u32,
    flags: u32,
    valid: bool,
}

const TLB_SIZE: usize = 64;

pub struct Mmu {
    pub enabled: bool,
    pub pdbr: u32,
    pub fault_addr: u32,
    pub fault_status: u32,
    pub kernel_sp: u32,
    tlb: [TlbEntry; TLB_SIZE],
}

impl Default for Mmu {
    fn default() -> Self {
        Self::new()
    }
}

impl Mmu {
    pub fn new() -> Self {
        Self {
            enabled: false,
            pdbr: 0,
            fault_addr: 0,
            fault_status: 0,
            kernel_sp: 0x7FFF_FFFF, // Default kernel stack base
            tlb: [TlbEntry::default(); TLB_SIZE],
        }
    }

    pub fn flush_tlb(&mut self) {
        for entry in &mut self.tlb {
            entry.valid = false;
        }
    }

    pub fn set_pdbr(&mut self, val: u32) {
        self.pdbr = val;
        self.flush_tlb();
    }

    pub fn set_enabled(&mut self, val: bool) {
        self.enabled = val;
        self.flush_tlb();
    }

    pub fn record_fault(&mut self, fault: MmuFault) {
        match fault {
            MmuFault::PageFault { vaddr, status } => {
                self.fault_addr = vaddr;
                self.fault_status = status;
            }
            MmuFault::PrivilegeViolation { vaddr } => {
                self.fault_addr = vaddr;
                self.fault_status = 3; // 3 = Privilege Violation
            }
        }
    }

    #[inline]
    fn tlb_lookup(&self, vpn: u32) -> Option<&TlbEntry> {
        let idx = (vpn as usize) % TLB_SIZE;
        let entry = &self.tlb[idx];
        if entry.valid && entry.vpn == vpn {
            Some(entry)
        } else {
            None
        }
    }

    #[inline]
    fn tlb_insert(&mut self, vpn: u32, pfn: u32, flags: u32) {
        let idx = (vpn as usize) % TLB_SIZE;
        self.tlb[idx] = TlbEntry {
            vpn,
            pfn,
            flags,
            valid: true,
        };
    }

    pub fn translate(
        &mut self,
        ram: &mut [u8],
        vaddr: u32,
        access: AccessType,
        is_user: bool,
    ) -> Result<u32, MmuFault> {
        if !self.enabled {
            return Ok(vaddr);
        }

        let vpn = vaddr >> 12;
        let offset = vaddr & (PAGE_SIZE - 1);

        // Check TLB cache
        if let Some(entry) = self.tlb_lookup(vpn) {
            let flags = entry.flags;
            // Check User permission
            if is_user && (flags & PTE_USER == 0) {
                let fault = MmuFault::PrivilegeViolation { vaddr };
                self.record_fault(fault);
                return Err(fault);
            }
            // Check Write permission
            if access == AccessType::Write && (flags & PTE_WRITABLE == 0) {
                let fault = MmuFault::PageFault {
                    vaddr,
                    status: AccessType::Write as u32,
                };
                self.record_fault(fault);
                return Err(fault);
            }
            // Check Exec permission
            if access == AccessType::Execute && (flags & PTE_EXEC == 0) {
                let fault = MmuFault::PageFault {
                    vaddr,
                    status: AccessType::Execute as u32,
                };
                self.record_fault(fault);
                return Err(fault);
            }

            return Ok(entry.pfn | offset);
        }

        // Two-level Page Table Walk
        let pdi = ((vaddr >> 22) & 0x3FF) as usize;
        let pti = ((vaddr >> 12) & 0x3FF) as usize;

        // 1. Page Directory Entry (PDE)
        let pde_addr = (self.pdbr as usize) + pdi * 4;
        if pde_addr + 4 > ram.len() {
            let fault = MmuFault::PageFault {
                vaddr,
                status: access as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        let pde = u32::from_be_bytes([
            ram[pde_addr],
            ram[pde_addr + 1],
            ram[pde_addr + 2],
            ram[pde_addr + 3],
        ]);

        if (pde & PTE_VALID) == 0 {
            let fault = MmuFault::PageFault {
                vaddr,
                status: access as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        if is_user && (pde & PTE_USER == 0) {
            let fault = MmuFault::PrivilegeViolation { vaddr };
            self.record_fault(fault);
            return Err(fault);
        }

        // 2. Page Table Entry (PTE)
        let pt_base = (pde & 0xFFFF_F000) as usize;
        let pte_addr = pt_base + pti * 4;
        if pte_addr + 4 > ram.len() {
            let fault = MmuFault::PageFault {
                vaddr,
                status: access as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        let mut pte = u32::from_be_bytes([
            ram[pte_addr],
            ram[pte_addr + 1],
            ram[pte_addr + 2],
            ram[pte_addr + 3],
        ]);

        if (pte & PTE_VALID) == 0 {
            let fault = MmuFault::PageFault {
                vaddr,
                status: access as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        if is_user && (pte & PTE_USER == 0) {
            let fault = MmuFault::PrivilegeViolation { vaddr };
            self.record_fault(fault);
            return Err(fault);
        }

        if access == AccessType::Write && (pte & PTE_WRITABLE == 0) {
            let fault = MmuFault::PageFault {
                vaddr,
                status: AccessType::Write as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        if access == AccessType::Execute && (pte & PTE_EXEC == 0) {
            let fault = MmuFault::PageFault {
                vaddr,
                status: AccessType::Execute as u32,
            };
            self.record_fault(fault);
            return Err(fault);
        }

        // Mark Accessed & Dirty bits
        pte |= PTE_ACCESSED;
        if access == AccessType::Write {
            pte |= PTE_DIRTY;
        }
        let pte_bytes = pte.to_be_bytes();
        ram[pte_addr..pte_addr + 4].copy_from_slice(&pte_bytes);

        let pfn = pte & 0xFFFF_F000;
        self.tlb_insert(vpn, pfn, pte);

        Ok(pfn | offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_page_table(ram: &mut [u8], pd_addr: u32, pt_addr: u32) {
        // Clear PD and PT
        for b in &mut ram[pd_addr as usize..pd_addr as usize + 4096] {
            *b = 0;
        }
        for b in &mut ram[pt_addr as usize..pt_addr as usize + 4096] {
            *b = 0;
        }

        // PD[0] points to PT (covers 0x0000_0000 .. 0x003F_FFFF)
        let pde = (pt_addr & 0xFFFF_F000) | PTE_VALID | PTE_WRITABLE | PTE_USER;
        let pde_bytes = pde.to_be_bytes();
        ram[pd_addr as usize..pd_addr as usize + 4].copy_from_slice(&pde_bytes);
    }

    #[test]
    fn translation_and_tlb() {
        let mut ram = vec![0u8; 1024 * 1024]; // 1 MB
        let mut mmu = Mmu::new();
        let pd_addr = 0x1000;
        let pt_addr = 0x2000;

        setup_test_page_table(&mut ram, pd_addr, pt_addr);

        // Virtual page 0x10 (vaddr = 0x10000) -> Physical frame 0x50000
        let pte = 0x50000 | PTE_VALID | PTE_WRITABLE | PTE_EXEC | PTE_USER;
        let pte_bytes = pte.to_be_bytes();
        let pte_addr = (pt_addr as usize) + 0x10 * 4;
        ram[pte_addr..pte_addr + 4].copy_from_slice(&pte_bytes);

        mmu.set_pdbr(pd_addr);
        mmu.set_enabled(true);

        // Translate read
        let paddr = mmu
            .translate(&mut ram, 0x10044, AccessType::Read, false)
            .expect("read should succeed");
        assert_eq!(paddr, 0x50044);

        // Accessed bit should be set in RAM
        let updated_pte = u32::from_be_bytes([
            ram[pte_addr],
            ram[pte_addr + 1],
            ram[pte_addr + 2],
            ram[pte_addr + 3],
        ]);
        assert_ne!(updated_pte & PTE_ACCESSED, 0);

        // Translate write (should hit TLB and set Dirty bit)
        let paddr_w = mmu
            .translate(&mut ram, 0x10048, AccessType::Write, true)
            .expect("write should succeed");
        assert_eq!(paddr_w, 0x50048);
    }

    #[test]
    fn page_fault_on_unmapped() {
        let mut ram = vec![0u8; 1024 * 1024];
        let mut mmu = Mmu::new();
        let pd_addr = 0x1000;
        let pt_addr = 0x2000;

        setup_test_page_table(&mut ram, pd_addr, pt_addr);
        mmu.set_pdbr(pd_addr);
        mmu.set_enabled(true);

        // Virtual address 0x20000 is not mapped in PT
        let res = mmu.translate(&mut ram, 0x20000, AccessType::Read, false);
        assert_eq!(
            res,
            Err(MmuFault::PageFault {
                vaddr: 0x20000,
                status: 0
            })
        );
    }

    #[test]
    fn readonly_write_fault() {
        let mut ram = vec![0u8; 1024 * 1024];
        let mut mmu = Mmu::new();
        let pd_addr = 0x1000;
        let pt_addr = 0x2000;

        setup_test_page_table(&mut ram, pd_addr, pt_addr);

        // Map read-only (no PTE_WRITABLE)
        let pte = 0x50000 | PTE_VALID | PTE_USER;
        let pte_addr = (pt_addr as usize) + 0x10 * 4;
        ram[pte_addr..pte_addr + 4].copy_from_slice(&pte.to_be_bytes());

        mmu.set_pdbr(pd_addr);
        mmu.set_enabled(true);

        let read_res = mmu.translate(&mut ram, 0x10000, AccessType::Read, true);
        assert!(read_res.is_ok());

        let write_res = mmu.translate(&mut ram, 0x10000, AccessType::Write, true);
        assert_eq!(
            write_res,
            Err(MmuFault::PageFault {
                vaddr: 0x10000,
                status: 1
            })
        );
    }

    #[test]
    fn privilege_violation_check() {
        let mut ram = vec![0u8; 1024 * 1024];
        let mut mmu = Mmu::new();
        let pd_addr = 0x1000;
        let pt_addr = 0x2000;

        setup_test_page_table(&mut ram, pd_addr, pt_addr);

        // Kernel-only page (no PTE_USER)
        let pte = 0x50000 | PTE_VALID | PTE_WRITABLE;
        let pte_addr = (pt_addr as usize) + 0x10 * 4;
        ram[pte_addr..pte_addr + 4].copy_from_slice(&pte.to_be_bytes());

        mmu.set_pdbr(pd_addr);
        mmu.set_enabled(true);

        // Kernel mode access succeeds
        assert!(mmu
            .translate(&mut ram, 0x10000, AccessType::Read, false)
            .is_ok());

        // User mode access triggers PrivilegeViolation
        assert_eq!(
            mmu.translate(&mut ram, 0x10000, AccessType::Read, true),
            Err(MmuFault::PrivilegeViolation { vaddr: 0x10000 })
        );
    }
}
