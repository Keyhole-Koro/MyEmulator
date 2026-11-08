// The instruction set, one row per instruction: its opcode (the top six bits
// of the word), the mnemonic traces print, and how many low bits of the word
// hold its immediate.
macro_rules! opcodes {
    ($($name:ident = $code:literal, $mnemonic:literal, $imm_bits:literal;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Opcode {
            $($name = $code,)+
        }

        impl TryFrom<u8> for Opcode {
            type Error = u8;
            fn try_from(val: u8) -> Result<Self, Self::Error> {
                match val {
                    $($code => Ok(Opcode::$name),)+
                    _ => Err(val),
                }
            }
        }

        impl Opcode {
            pub fn mnemonic(self) -> &'static str {
                match self {
                    $(Opcode::$name => $mnemonic,)+
                }
            }

            pub fn imm_bits(self) -> u32 {
                match self {
                    $(Opcode::$name => $imm_bits,)+
                }
            }
        }
    };
}

opcodes! {
    Mov = 0x01, "MOV", 16;
    Movi = 0x02, "MOVI", 21;
    Movis = 0x18, "MOVIS", 21;
    Ld = 0x03, "LD", 16;
    St = 0x04, "ST", 16;
    Ldb = 0x1C, "LDB", 16;
    Stb = 0x1D, "STB", 16;
    Add = 0x05, "ADD", 16;
    Addis = 0x19, "ADDIS", 21;
    Sub = 0x06, "SUB", 16;
    Cmp = 0x07, "CMP", 16;
    And = 0x08, "AND", 16;
    Or = 0x09, "OR", 16;
    Xor = 0x0A, "XOR", 16;
    Shl = 0x0B, "SHL", 16;
    Shr = 0x0C, "SHR", 16;
    Jmp = 0x0D, "JMP", 26;
    Jz = 0x0E, "JZ", 26;
    Jnz = 0x0F, "JNZ", 26;
    Jg = 0x10, "JG", 26;
    Jl = 0x11, "JL", 26;
    Ja = 0x12, "JA", 26;
    Jb = 0x13, "JB", 26;
    Call = 0x1B, "CALL", 26;
    Push = 0x14, "PUSH", 16;
    Pop = 0x15, "POP", 16;
    In = 0x16, "IN", 21;
    Out = 0x17, "OUT", 21;
    Debug = 0x1A, "DEBUG", 16;
    Ei = 0x1E, "EI", 16;
    Di = 0x1F, "DI", 16;
    Iret = 0x20, "IRET", 16;
    Wfi = 0x21, "WFI", 16;
    Swp = 0x22, "SWP", 16;
    Cas = 0x23, "CAS", 16;
    Chk = 0x24, "CHK", 21;
    Syscall = 0x3E, "SYSCALL", 16;
    Halt = 0x3F, "HALT", 16;
}

#[derive(Clone, Copy)]
pub struct DecodedInstruction {
    // None when the opcode bits name no instruction.
    pub opcode: Option<Opcode>,
    pub reg1: u8,
    pub reg2: u8,
    pub reg3: u8,
    pub imm: u32,
    pub raw: u32,
}

impl DecodedInstruction {
    // The opcode field, whether or not it names an instruction.
    pub fn opcode_bits(&self) -> u8 {
        (self.raw >> 26) as u8
    }

    pub fn mnemonic(&self) -> &'static str {
        self.opcode.map_or("UNKNOWN", Opcode::mnemonic)
    }
}

pub fn sign_extend_26(x: u32) -> i32 {
    ((x << 6) as i32) >> 6
}

pub fn sign_extend_21(x: u32) -> i32 {
    ((x << 11) as i32) >> 11
}

pub fn add_signed(base: u32, delta: i32) -> u32 {
    if delta >= 0 {
        base.wrapping_add(delta as u32)
    } else {
        base.wrapping_sub((-delta) as u32)
    }
}

pub fn decode_instruction(machine_code: u32) -> DecodedInstruction {
    let opcode = Opcode::try_from((machine_code >> 26) as u8).ok();
    // An unknown opcode still shows a 16-bit immediate in traces.
    let imm_bits = opcode.map_or(16, Opcode::imm_bits);

    DecodedInstruction {
        opcode,
        reg1: ((machine_code >> 21) & 0x1F) as u8,
        reg2: ((machine_code >> 16) & 0x1F) as u8,
        reg3: ((machine_code >> 11) & 0x1F) as u8,
        imm: machine_code & ((1 << imm_bits) - 1),
        raw: machine_code,
    }
}

// The mnemonic of an opcode field, for reports that keep the raw bits.
pub fn mnemonic(opcode: u8) -> &'static str {
    Opcode::try_from(opcode).map_or("UNKNOWN", Opcode::mnemonic)
}

#[cfg(test)]
mod tests {
    use super::{decode_instruction, mnemonic, Opcode};

    const ALL_LOW_BITS: u32 = 0x03FF_FFFF;

    #[test]
    fn each_opcode_decodes_to_itself() {
        let known: Vec<u8> = (0..64)
            .filter(|&code| Opcode::try_from(code).is_ok())
            .collect();
        assert_eq!(known.len(), 38);
        for code in known {
            let inst = decode_instruction(u32::from(code) << 26);
            assert_eq!(inst.opcode.map(|op| op as u8), Some(code));
            assert_eq!(inst.mnemonic(), mnemonic(code));
        }
    }

    #[test]
    fn the_immediate_width_follows_the_opcode() {
        for (op, imm) in [
            (Opcode::Movi, 0x1F_FFFF),
            (Opcode::Chk, 0x1F_FFFF),
            (Opcode::Call, 0x3FF_FFFF),
            (Opcode::Jnz, 0x3FF_FFFF),
            (Opcode::Add, 0xFFFF),
        ] {
            let inst = decode_instruction(((op as u32) << 26) | ALL_LOW_BITS);
            assert_eq!(inst.imm, imm, "{}", op.mnemonic());
        }
        let unknown = decode_instruction((0x3D << 26) | ALL_LOW_BITS);
        assert_eq!(unknown.opcode, None);
        assert_eq!(unknown.opcode_bits(), 0x3D);
        assert_eq!(unknown.mnemonic(), "UNKNOWN");
        assert_eq!(unknown.imm, 0xFFFF);
    }
}
