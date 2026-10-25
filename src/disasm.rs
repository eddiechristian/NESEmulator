use crate::bus::Bus;
use crate::cpu::IMPLEMENTED;
use crate::opcode::OPCODES_MAP;

// Addressing modes as they appear in assembly syntax. Separate from cpu::AddressingMode
// because disassembly also needs Accumulator, Indirect and Relative
#[derive(Clone, Copy)]
enum Mode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,
}

impl Mode {
    fn len(self) -> u16 {
        match self {
            Mode::Implied | Mode::Accumulator => 1,
            Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY | Mode::Indirect => 3,
            _ => 2,
        }
    }

    // olcNES's three-letter names. olc has no separate accumulator mode, so ASL A etc. show as IMP
    fn abbr(self) -> &'static str {
        match self {
            Mode::Implied | Mode::Accumulator => "IMP",
            Mode::Immediate => "IMM",
            Mode::ZeroPage => "ZP0",
            Mode::ZeroPageX => "ZPX",
            Mode::ZeroPageY => "ZPY",
            Mode::Absolute => "ABS",
            Mode::AbsoluteX => "ABX",
            Mode::AbsoluteY => "ABY",
            Mode::Indirect => "IND",
            Mode::IndirectX => "IZX",
            Mode::IndirectY => "IZY",
            Mode::Relative => "REL",
        }
    }
}

// Every official 6502 opcode, including ones the CPU doesn't implement yet,
// so the listing can step over them with the right length
#[rustfmt::skip]
const ALL_OPCODES: &[(u8, &str, Mode)] = &[
    (0x69, "ADC", Mode::Immediate), (0x65, "ADC", Mode::ZeroPage), (0x75, "ADC", Mode::ZeroPageX), (0x6D, "ADC", Mode::Absolute),
    (0x7D, "ADC", Mode::AbsoluteX), (0x79, "ADC", Mode::AbsoluteY), (0x61, "ADC", Mode::IndirectX), (0x71, "ADC", Mode::IndirectY),
    (0x29, "AND", Mode::Immediate), (0x25, "AND", Mode::ZeroPage), (0x35, "AND", Mode::ZeroPageX), (0x2D, "AND", Mode::Absolute),
    (0x3D, "AND", Mode::AbsoluteX), (0x39, "AND", Mode::AbsoluteY), (0x21, "AND", Mode::IndirectX), (0x31, "AND", Mode::IndirectY),
    (0x0A, "ASL", Mode::Accumulator), (0x06, "ASL", Mode::ZeroPage), (0x16, "ASL", Mode::ZeroPageX), (0x0E, "ASL", Mode::Absolute),
    (0x1E, "ASL", Mode::AbsoluteX),
    (0x90, "BCC", Mode::Relative), (0xB0, "BCS", Mode::Relative), (0xF0, "BEQ", Mode::Relative), (0x30, "BMI", Mode::Relative),
    (0xD0, "BNE", Mode::Relative), (0x10, "BPL", Mode::Relative), (0x50, "BVC", Mode::Relative), (0x70, "BVS", Mode::Relative),
    (0x24, "BIT", Mode::ZeroPage), (0x2C, "BIT", Mode::Absolute),
    (0x00, "BRK", Mode::Implied),
    (0x18, "CLC", Mode::Implied), (0xD8, "CLD", Mode::Implied), (0x58, "CLI", Mode::Implied), (0xB8, "CLV", Mode::Implied),
    (0xC9, "CMP", Mode::Immediate), (0xC5, "CMP", Mode::ZeroPage), (0xD5, "CMP", Mode::ZeroPageX), (0xCD, "CMP", Mode::Absolute),
    (0xDD, "CMP", Mode::AbsoluteX), (0xD9, "CMP", Mode::AbsoluteY), (0xC1, "CMP", Mode::IndirectX), (0xD1, "CMP", Mode::IndirectY),
    (0xE0, "CPX", Mode::Immediate), (0xE4, "CPX", Mode::ZeroPage), (0xEC, "CPX", Mode::Absolute),
    (0xC0, "CPY", Mode::Immediate), (0xC4, "CPY", Mode::ZeroPage), (0xCC, "CPY", Mode::Absolute),
    (0xC6, "DEC", Mode::ZeroPage), (0xD6, "DEC", Mode::ZeroPageX), (0xCE, "DEC", Mode::Absolute), (0xDE, "DEC", Mode::AbsoluteX),
    (0xCA, "DEX", Mode::Implied), (0x88, "DEY", Mode::Implied),
    (0x49, "EOR", Mode::Immediate), (0x45, "EOR", Mode::ZeroPage), (0x55, "EOR", Mode::ZeroPageX), (0x4D, "EOR", Mode::Absolute),
    (0x5D, "EOR", Mode::AbsoluteX), (0x59, "EOR", Mode::AbsoluteY), (0x41, "EOR", Mode::IndirectX), (0x51, "EOR", Mode::IndirectY),
    (0xE6, "INC", Mode::ZeroPage), (0xF6, "INC", Mode::ZeroPageX), (0xEE, "INC", Mode::Absolute), (0xFE, "INC", Mode::AbsoluteX),
    (0xE8, "INX", Mode::Implied), (0xC8, "INY", Mode::Implied),
    (0x4C, "JMP", Mode::Absolute), (0x6C, "JMP", Mode::Indirect),
    (0x20, "JSR", Mode::Absolute),
    (0xA9, "LDA", Mode::Immediate), (0xA5, "LDA", Mode::ZeroPage), (0xB5, "LDA", Mode::ZeroPageX), (0xAD, "LDA", Mode::Absolute),
    (0xBD, "LDA", Mode::AbsoluteX), (0xB9, "LDA", Mode::AbsoluteY), (0xA1, "LDA", Mode::IndirectX), (0xB1, "LDA", Mode::IndirectY),
    (0xA2, "LDX", Mode::Immediate), (0xA6, "LDX", Mode::ZeroPage), (0xB6, "LDX", Mode::ZeroPageY), (0xAE, "LDX", Mode::Absolute),
    (0xBE, "LDX", Mode::AbsoluteY),
    (0xA0, "LDY", Mode::Immediate), (0xA4, "LDY", Mode::ZeroPage), (0xB4, "LDY", Mode::ZeroPageX), (0xAC, "LDY", Mode::Absolute),
    (0xBC, "LDY", Mode::AbsoluteX),
    (0x4A, "LSR", Mode::Accumulator), (0x46, "LSR", Mode::ZeroPage), (0x56, "LSR", Mode::ZeroPageX), (0x4E, "LSR", Mode::Absolute),
    (0x5E, "LSR", Mode::AbsoluteX),
    (0xEA, "NOP", Mode::Implied),
    (0x09, "ORA", Mode::Immediate), (0x05, "ORA", Mode::ZeroPage), (0x15, "ORA", Mode::ZeroPageX), (0x0D, "ORA", Mode::Absolute),
    (0x1D, "ORA", Mode::AbsoluteX), (0x19, "ORA", Mode::AbsoluteY), (0x01, "ORA", Mode::IndirectX), (0x11, "ORA", Mode::IndirectY),
    (0x48, "PHA", Mode::Implied), (0x08, "PHP", Mode::Implied), (0x68, "PLA", Mode::Implied), (0x28, "PLP", Mode::Implied),
    (0x2A, "ROL", Mode::Accumulator), (0x26, "ROL", Mode::ZeroPage), (0x36, "ROL", Mode::ZeroPageX), (0x2E, "ROL", Mode::Absolute),
    (0x3E, "ROL", Mode::AbsoluteX),
    (0x6A, "ROR", Mode::Accumulator), (0x66, "ROR", Mode::ZeroPage), (0x76, "ROR", Mode::ZeroPageX), (0x6E, "ROR", Mode::Absolute),
    (0x7E, "ROR", Mode::AbsoluteX),
    (0x40, "RTI", Mode::Implied), (0x60, "RTS", Mode::Implied),
    (0xE9, "SBC", Mode::Immediate), (0xE5, "SBC", Mode::ZeroPage), (0xF5, "SBC", Mode::ZeroPageX), (0xED, "SBC", Mode::Absolute),
    (0xFD, "SBC", Mode::AbsoluteX), (0xF9, "SBC", Mode::AbsoluteY), (0xE1, "SBC", Mode::IndirectX), (0xF1, "SBC", Mode::IndirectY),
    (0x38, "SEC", Mode::Implied), (0xF8, "SED", Mode::Implied), (0x78, "SEI", Mode::Implied),
    (0x85, "STA", Mode::ZeroPage), (0x95, "STA", Mode::ZeroPageX), (0x8D, "STA", Mode::Absolute), (0x9D, "STA", Mode::AbsoluteX),
    (0x99, "STA", Mode::AbsoluteY), (0x81, "STA", Mode::IndirectX), (0x91, "STA", Mode::IndirectY),
    (0x86, "STX", Mode::ZeroPage), (0x96, "STX", Mode::ZeroPageY), (0x8E, "STX", Mode::Absolute),
    (0x84, "STY", Mode::ZeroPage), (0x94, "STY", Mode::ZeroPageX), (0x8C, "STY", Mode::Absolute),
    (0xAA, "TAX", Mode::Implied), (0xA8, "TAY", Mode::Implied), (0xBA, "TSX", Mode::Implied), (0x8A, "TXA", Mode::Implied),
    (0x9A, "TXS", Mode::Implied), (0x98, "TYA", Mode::Implied),
];

fn lookup(code: u8) -> Option<(&'static str, Mode)> {
    ALL_OPCODES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, name, mode)| (*name, *mode))
}

// Formats the operand the way an assembler would write it, e.g. #$00, $2002,X, ($12),Y
fn format_operand(mode: Mode, addr: u16, lo: u8, hi: u8) -> String {
    let word = u16::from_le_bytes([lo, hi]);
    match mode {
        Mode::Implied => String::new(),
        Mode::Accumulator => "A".to_string(),
        Mode::Immediate => format!("#${:02X}", lo),
        Mode::ZeroPage => format!("${:02X}", lo),
        Mode::ZeroPageX => format!("${:02X},X", lo),
        Mode::ZeroPageY => format!("${:02X},Y", lo),
        Mode::Absolute => format!("${:04X}", word),
        Mode::AbsoluteX => format!("${:04X},X", word),
        Mode::AbsoluteY => format!("${:04X},Y", word),
        Mode::Indirect => format!("(${:04X})", word),
        Mode::IndirectX => format!("(${:02X},X)", lo),
        Mode::IndirectY => format!("(${:02X}),Y", lo),
        // Show the branch target rather than the raw signed offset
        Mode::Relative => format!(
            "${:04X}",
            addr.wrapping_add(2).wrapping_add(lo as i8 as u16)
        ),
    }
}

// One decoded instruction, for callers that want to lay out the listing themselves
pub struct Instruction {
    pub addr: u16,
    pub len: u16,
    pub bytes: String,      // raw bytes in hex, e.g. "AD 02 20"
    pub text: String,       // assembly, e.g. "LDA $2002"
    pub mode: &'static str, // olcNES mode abbreviation, e.g. "ABS"
    // Needs both an opcode table entry and a handler in CPU::execute
    pub implemented: bool,
}

pub fn decode(bus: &Bus, addr: u16) -> Instruction {
    let read = |offset: u16| bus.read(addr.wrapping_add(offset));
    let code = read(0);

    let (len, text, mode, implemented) = match lookup(code) {
        Some((name, mode)) => {
            let operand = format_operand(mode, addr, read(1), read(2));
            let implemented = OPCODES_MAP.contains_key(&code) && IMPLEMENTED.contains(&name);
            (
                mode.len(),
                format!("{} {}", name, operand).trim_end().to_string(),
                mode.abbr(),
                implemented,
            )
        }
        None => (1, format!(".db ${:02X}", code), "???", false),
    };

    let bytes = (0..len)
        .map(|i| format!("{:02X}", read(i)))
        .collect::<Vec<_>>()
        .join(" ");

    Instruction {
        addr,
        len,
        bytes,
        text,
        mode,
        implemented,
    }
}

// Linear-sweep decode of `count` instructions starting at `start`
pub fn decode_range(bus: &Bus, start: u16, count: usize) -> Vec<Instruction> {
    let mut out = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        let ins = decode(bus, addr);
        addr = addr.wrapping_add(ins.len);
        out.push(ins);
    }
    out
}

// Linear-sweep disassembly of `count` instructions starting at `start`, marking which
// ones the CPU can execute. Returns one formatted line per instruction.
// Doesn't follow jumps, so data mixed into code will show up as garbage
pub fn disassemble(bus: &Bus, start: u16, count: usize) -> Vec<String> {
    let mut first_missing = true;
    decode_range(bus, start, count)
        .into_iter()
        .map(|ins| {
            let status = if ins.implemented {
                "✓ implemented"
            } else if first_missing {
                first_missing = false;
                "✗ missing  <- next to implement"
            } else {
                "✗ missing"
            };
            format!(
                "{:04X}  {:<8}  {:<13} {}  {}",
                ins.addr, ins.bytes, ins.text, ins.mode, status
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::AddressingMode as Cpu;

    // The CPU's opcode table (opcode.rs) and this one are written separately, so check
    // every official opcode has the same mnemonic, length and mode in both
    #[test]
    fn opcode_table_matches_disassembler() {
        assert_eq!(ALL_OPCODES.len(), 151);
        for &(code, name, mode) in ALL_OPCODES {
            let op = OPCODES_MAP
                .get(&code)
                .unwrap_or_else(|| panic!("${:02X} {} missing from opcode.rs", code, name));
            assert_eq!(op.mnemonic, name, "mnemonic for ${:02X}", code);
            assert_eq!(
                op.len as u16,
                mode.len(),
                "length for ${:02X} {}",
                code,
                name
            );
            let same_mode = matches!(
                (mode, &op.mode),
                (Mode::Implied, Cpu::NoneAddressing)
                    | (Mode::Accumulator, Cpu::Accumulator)
                    | (Mode::Immediate, Cpu::Immediate)
                    | (Mode::ZeroPage, Cpu::ZeroPage)
                    | (Mode::ZeroPageX, Cpu::ZeroPage_X)
                    | (Mode::ZeroPageY, Cpu::ZeroPage_Y)
                    | (Mode::Absolute, Cpu::Absolute)
                    | (Mode::AbsoluteX, Cpu::Absolute_X)
                    | (Mode::AbsoluteY, Cpu::Absolute_Y)
                    | (Mode::Indirect, Cpu::Indirect)
                    | (Mode::IndirectX, Cpu::Indirect_X)
                    | (Mode::IndirectY, Cpu::Indirect_Y)
                    | (Mode::Relative, Cpu::Relative)
            );
            assert!(same_mode, "mode for ${:02X} {}: {:?}", code, name, op.mode);
            assert!(IMPLEMENTED.contains(&name), "{} not in IMPLEMENTED", name);
        }
        assert_eq!(
            OPCODES_MAP.len(),
            151,
            "opcode.rs has entries the disassembler doesn't"
        );
    }
}
