use crate::memory::Mem;
use std::fmt;

use crate::opcode::{OPCODES_MAP, OpCode};

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
            .field("sr", &format_args!("0x{:02x}", self.sr))
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

    fn ldx(&mut self, operands: &[u8]) {
        println!("ldx operands: {:#04x?}", operands);
        self.reg_x = operands[0];
    }

    fn lda(&mut self, operands: &[u8]) {
        println!("lda operands: {:#04x?}", operands);
        self.reg_a = operands[0];
    }

    fn sta(&mut self, operands: &[u8], mem: &mut Mem) {
        println!("sta operands: {:#04x?}", operands);
        // Zero page,X: address wraps within page zero
        let addr = operands[0].wrapping_add(self.reg_x) as usize;

        // Print before modification
        println!("Memory at {:#06x} BEFORE: {:#04x}", addr, mem.mem[addr]);

        mem.mem[addr] = self.reg_a;

        // Print after modification
        println!("Memory at {:#06x} AFTER:  {:#04x}", addr, mem.mem[addr]);
    }

    pub fn step(&mut self, steps: usize, mem: &mut Mem) {
        for i in 0..steps {
            let code = mem.mem[self.pc as usize];
            let opcode = OPCODES_MAP
                .get(&code)
                .expect(&format!("OpCode {:x} is not recognized", code));
            println!("opcode  code:{:#04x} len: {}", opcode.code, opcode.len);

            match opcode.mnemonic {
                "LDX" => {
                    let operands = match opcode.mode {
                        AddressingMode::Immediate => {
                            &mem.mem[self.pc + 1..self.pc + opcode.len as usize]
                        }
                        _ => &mem.mem[self.pc + 1..self.pc + 2],
                    };

                    self.ldx(operands);
                }
                "LDA" => {
                    let operands = match opcode.mode {
                        AddressingMode::Immediate => {
                            &mem.mem[self.pc + 1..self.pc + opcode.len as usize]
                        }
                        _ => &mem.mem[self.pc + 1..self.pc + 2],
                    };

                    self.lda(operands);
                }
                "STA" => {
                    // Copy operands out so `mem` isn't still borrowed when passed mutably
                    let operands = match opcode.mode {
                        AddressingMode::Immediate => {
                            mem.mem[self.pc + 1..self.pc + opcode.len as usize].to_vec()
                        }
                        _ => mem.mem[self.pc + 1..self.pc + 2].to_vec(),
                    };

                    self.sta(&operands, mem);
                }
                _ => {
                    eprintln!("no mneumic")
                }
            }
            self.pc += opcode.len as usize;
            println!("{:#X}", self);
        }
    }
}
