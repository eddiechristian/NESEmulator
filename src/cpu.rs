use crate::bus::Bus;
use std::fmt;

use crate::opcode::{OPCODES_MAP, OpCode};

// Mnemonics that CPU::step has a handler for. Keep in sync with the match in step
pub const IMPLEMENTED: &[&str] = &["CLD", "SEI", "JSR", "RTS", "LDX", "LDA", "STA", "BPL"];

const STACK_START: u16 = 0x0100;
const STACK_END: u16 = 0x01FF;

// Status register bits: NV-B DIZC
const FLAG_CARRY: u8 = 0b0000_0001;
const FLAG_ZERO: u8 = 0b0000_0010;
const FLAG_INTERRUPT_DISABLE: u8 = 0b0000_0100;
const FLAG_DECIMAL: u8 = 0b0000_1000;
const FLAG_BREAK: u8 = 0b0001_0000;
const FLAG_UNUSED: u8 = 0b0010_0000;
const FLAG_OVERFLOW: u8 = 0b0100_0000;
const FLAG_NEGATIVE: u8 = 0b1000_0000;

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum AddressingMode {
    Immediate,
    ZeroPage,
    ZeroPage_X,
    ZeroPage_Y,
    Absolute,
    Absolute_X,
    Absolute_Y,
    Indirect_X,
    Indirect_Y,
    NoneAddressing,
}

pub struct CPU {
    pub reg_a: u8,
    pub reg_x: u8,
    pub reg_y: u8,
    pub sp: u8,
    pub pc: usize,
    pub sr: u8,
}

impl fmt::UpperHex for CPU {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CPU")
            .field("reg_a", &format_args!("0x{:02x}", self.reg_a))
            .field("reg_x", &format_args!("0x{:02x}", self.reg_x))
            .field("reg_y", &format_args!("0x{:02x}", self.reg_y))
            .field("sp", &format_args!("0x{:02x}", self.sp))
            // PC is a usize, so 4 hex digits (0x0000) fits typical 16-bit emulators
            .field("pc", &format_args!("0x{:04x}", self.pc))
            .field("sr", &format_args!("b{:08b}", self.sr))
            .finish()
    }
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            reg_a: 0x00,
            reg_x: 0x00,
            reg_y: 0x00,
            sp: 0xFF,
            pc: 0x0600,
            sr: 0b0011_0000,
        }
    }

    // Like powering on a real NES: start wherever the reset vector at $FFFC/$FFFD points
    pub fn reset(&mut self, bus: &Bus) {
        self.pc = u16::from_le_bytes([bus.read(0xFFFC), bus.read(0xFFFD)]) as usize;
        self.sp = 0xFD;
        self.sr = 0b0010_0100; // interrupt disable set
        println!("reset vector: {:#06x}", self.pc);
    }

    // Sets or clears a single status register bit
    fn set_flag(&mut self, flag: u8, on: bool) {
        if on {
            self.sr |= flag;
        } else {
            self.sr &= !flag;
        }
    }

    pub fn set_carry(&mut self, on: bool) {
        self.set_flag(FLAG_CARRY, on);
    }

    pub fn set_zero(&mut self, on: bool) {
        self.set_flag(FLAG_ZERO, on);
    }

    pub fn set_interrupt_disable(&mut self, on: bool) {
        self.set_flag(FLAG_INTERRUPT_DISABLE, on);
    }

    pub fn set_decimal(&mut self, on: bool) {
        self.set_flag(FLAG_DECIMAL, on);
    }

    pub fn set_break(&mut self, on: bool) {
        self.set_flag(FLAG_BREAK, on);
    }

    // Bit 5 has no function on the 6502 and normally reads as 1
    pub fn set_unused(&mut self, on: bool) {
        self.set_flag(FLAG_UNUSED, on);
    }

    pub fn set_overflow(&mut self, on: bool) {
        self.set_flag(FLAG_OVERFLOW, on);
    }

    pub fn set_negative(&mut self, on: bool) {
        self.set_flag(FLAG_NEGATIVE, on);
    }

    // The stack lives in page one ($0100-$01FF) and grows downward
    fn pushb(&mut self, bus: &mut Bus, value: u8) {
        bus.write(STACK_START + self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    // 6502 pushes the high byte first, so the low byte ends up at the lower address
    fn pushw(&mut self, bus: &mut Bus, value: u16) {
        self.pushb(bus, (value >> 8) as u8);
        self.pushb(bus, (value & 0xFF) as u8);
    }

    // Pop is the mirror of push: increment SP first, then read
    fn popb(&mut self, bus: &Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(STACK_START + self.sp as u16)
    }

    // Low byte comes off first since it was pushed last
    fn popw(&mut self, bus: &Bus) -> u16 {
        let lo = self.popb(bus);
        let hi = self.popb(bus);
        u16::from_le_bytes([lo, hi])
    }

    // Prints the used part of the stack, from the top (most recent push) down to $01FF
    pub fn print_stack(&self, bus: &Bus) {
        println!("--- Stack (SP = {:#04x}) ---", self.sp);
        let top = STACK_START + self.sp as u16 + 1;
        if top > STACK_END {
            println!("  (empty)");
            return;
        }
        for addr in top..=STACK_END {
            println!("  {:#06x}: {:#04x}", addr, bus.read(addr));
        }
    }


    // Works out the effective address an instruction's operand refers to.
    // For Immediate, that's the operand byte itself (PC + 1).
    fn operand_address(&self, mode: &AddressingMode, bus: &Bus) -> u16 {
        let arg = bus.read(self.pc as u16 + 1);
        // Reads a little-endian pointer from page zero; the high byte wraps within page zero
        let zp_word = |ptr: u8| {
            u16::from_le_bytes([bus.read(ptr as u16), bus.read(ptr.wrapping_add(1) as u16)])
        };
        let abs = || u16::from_le_bytes([arg, bus.read(self.pc as u16 + 2)]);

        match mode {
            AddressingMode::Immediate => self.pc as u16 + 1,
            AddressingMode::ZeroPage => arg as u16,
            // Zero page indexed addresses wrap within page zero
            AddressingMode::ZeroPage_X => arg.wrapping_add(self.reg_x) as u16,
            AddressingMode::ZeroPage_Y => arg.wrapping_add(self.reg_y) as u16,
            AddressingMode::Absolute => abs(),
            AddressingMode::Absolute_X => abs().wrapping_add(self.reg_x as u16),
            AddressingMode::Absolute_Y => abs().wrapping_add(self.reg_y as u16),
            // (zp,X): add X to the zero page address first, then read the pointer there
            AddressingMode::Indirect_X => zp_word(arg.wrapping_add(self.reg_x)),
            // (zp),Y: read the pointer from zero page first, then add Y to it
            AddressingMode::Indirect_Y => zp_word(arg).wrapping_add(self.reg_y as u16),
            AddressingMode::NoneAddressing => {
                panic!("mode {:?} has no operand address", mode)
            }
        }
    }

    // Z is set when the result is zero, N copies bit 7 of the result
    fn update_zero_and_negative(&mut self, result: u8) {
        self.set_zero(result == 0);
        self.set_negative(result & 0x80 != 0);
    }

    fn lda(&mut self, mode: &AddressingMode, bus: &Bus) {
        let addr = self.operand_address(mode, bus);
        let value = bus.read(addr);
        println!("lda {:?} addr {:#06x} value {:#04x}", mode, addr, value);
        self.reg_a = value;
        self.update_zero_and_negative(value);
    }

    fn ldx(&mut self, mode: &AddressingMode, bus: &Bus) {
        let addr = self.operand_address(mode, bus);
        let value = bus.read(addr);
        println!("ldx {:?} addr {:#06x} value {:#04x}", mode, addr, value);
        self.reg_x = value;
        self.update_zero_and_negative(value);
    }

    fn sta(&mut self, mode: &AddressingMode, bus: &mut Bus) {
        let addr = self.operand_address(mode, bus);
        println!("sta {:?} addr {:#06x}", mode, addr);

        // Print before modification
        println!("Memory at {:#06x} BEFORE: {:#04x}", addr, bus.read(addr));

        bus.write(addr, self.reg_a);

        // Print after modification
        println!("Memory at {:#06x} AFTER:  {:#04x}", addr, bus.read(addr));
    }

    // Branch operand is a signed offset from the next instruction (PC + 2)
    fn branch(&mut self, condition: bool, bus: &Bus) {
        let offset = bus.read(self.pc as u16 + 1) as i8;
        let target = (self.pc as u16).wrapping_add(2).wrapping_add(offset as u16);
        println!("branch offset {} to {:#06x}, taken: {}", offset, target, condition);
        if condition {
            self.pc = target as usize;
        }
    }

    fn jsr(&mut self, operands: &[u8], bus: &mut Bus) {
        // JSR pushes the address of its own last byte (PC + 2), not the next instruction;
        // RTS adds 1 after pulling it
        self.print_stack(bus);
        let return_addr = (self.pc + 2) as u16;
        self.pushw(bus, return_addr);

        self.print_stack(bus);
        // Operand is a little-endian absolute address
        let target = u16::from_le_bytes([operands[0], operands[1]]);
        println!("jsr to {:#06x}, return addr {:#06x}", target, return_addr);
        self.pc = target as usize;
    }

    fn rts(&mut self, bus: &Bus) {
        self.print_stack(bus);
        // JSR pushed the address of its own last byte, so add 1 to land on the next instruction
        let return_addr = self.popw(bus).wrapping_add(1);
        self.print_stack(bus);
        println!("rts to {:#06x}", return_addr);
        self.pc = return_addr as usize;
    }

    pub fn step(&mut self, steps: usize, bus: &mut Bus) {
        for i in 0..steps {
            let code = bus.read(self.pc as u16);
            let opcode = OPCODES_MAP
                .get(&code)
                .expect(&format!("OpCode {:x} is not recognized", code));
            println!("opcode  code:{:#04x} len: {}", opcode.code, opcode.len);
            let pc_before = self.pc;

            match opcode.mnemonic {
                "CLD" => {
                    self.set_decimal(false);
                }
                "SEI" => {
                    self.set_interrupt_disable(true);
                }
                "JSR" => {
                    let operands: Vec<u8> = (1..opcode.len as u16)
                        .map(|i| bus.read(self.pc as u16 + i))
                        .collect();
                    self.jsr(&operands, bus);
                }
                "RTS" => {
                    self.rts(bus);
                }
                "LDX" => {
                    self.ldx(&opcode.mode, bus);
                }
                "LDA" => {
                    self.lda(&opcode.mode, bus);
                }
                "STA" => {
                    self.sta(&opcode.mode, bus);
                }
                "BPL" => {
                    self.branch(self.sr & FLAG_NEGATIVE == 0, bus);
                }
                _ => {
                    eprintln!("no mneumic")
                }
            }
            // Only advance if the instruction didn't jump
            if self.pc == pc_before {
                self.pc += opcode.len as usize;
            }
            println!("{:#X}", self);
        }
    }
}
