#[derive(Clone, Copy)]
pub struct DecodedInstruction {
    pub opcode: u8,
    pub reg1: u8,
    pub reg2: u8,
    pub imm: u32,
    pub raw: u32,
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

fn imm_mask(opcode: u8) -> u32 {
    match opcode {
        0x02 | 0x18 | 0x19 | 0x16 | 0x17 => 0x001F_FFFF,
        0x0D | 0x0E | 0x0F | 0x10 | 0x11 | 0x12 | 0x13 | 0x1B => 0x03FF_FFFF,
        _ => 0x0000_FFFF,
    }
}

pub fn decode_instruction(machine_code: u32) -> DecodedInstruction {
    let opcode = ((machine_code >> 26) & 0x3F) as u8;
    let reg1 = ((machine_code >> 21) & 0x1F) as u8;
    let reg2 = ((machine_code >> 16) & 0x1F) as u8;
    let imm = machine_code & imm_mask(opcode);

    DecodedInstruction {
        opcode,
        reg1,
        reg2,
        imm,
        raw: machine_code,
    }
}

pub fn mnemonic(opcode: u8) -> &'static str {
    match opcode {
        0x01 => "MOV",
        0x02 => "MOVI",
        0x18 => "MOVIS",
        0x03 => "LD",
        0x04 => "ST",
        0x1C => "LDB",
        0x1D => "STB",
        0x05 => "ADD",
        0x19 => "ADDIS",
        0x06 => "SUB",
        0x07 => "CMP",
        0x08 => "AND",
        0x09 => "OR",
        0x0A => "XOR",
        0x0B => "SHL",
        0x0C => "SHR",
        0x0D => "JMP",
        0x0E => "JZ",
        0x0F => "JNZ",
        0x10 => "JG",
        0x11 => "JL",
        0x12 => "JA",
        0x13 => "JB",
        0x1B => "CALL",
        0x14 => "PUSH",
        0x15 => "POP",
        0x16 => "IN",
        0x17 => "OUT",
        0x1A => "DEBUG",
        0x1E => "EI",
        0x1F => "DI",
        0x20 => "IRET",
        0x3F => "HALT",
        _ => "UNKNOWN",
    }
}
