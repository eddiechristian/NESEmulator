use crate::bus::Bus;
use std::fmt;

use crate::opcode::{OPCODES_MAP, OpCode};

// Mnemonics that CPU::execute has a handler for. Keep in sync with the match in execute
pub const IMPLEMENTED: &[&str] = &[
    "BPL", "CLD", "CMP", "DEX", "SEI", "JSR", "RTS", "LDX", "LDA", "NOP", "STA", "STX", "TAX",
    "TAY", "TXS",
];

const STACK_START: u16 = 0x0100;
const STACK_END: u16 = 0x01FF;

// Status register bits: NV-B DIZC
pub const FLAG_CARRY: u8 = 0b0000_0001;
pub const FLAG_ZERO: u8 = 0b0000_0010;
pub const FLAG_INTERRUPT_DISABLE: u8 = 0b0000_0100;
pub const FLAG_DECIMAL: u8 = 0b0000_1000;
pub const FLAG_BREAK: u8 = 0b0001_0000;
pub const FLAG_UNUSED: u8 = 0b0010_0000;
pub const FLAG_OVERFLOW: u8 = 0b0100_0000;
pub const FLAG_NEGATIVE: u8 = 0b1000_0000;

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
    // Cycles left before the current instruction finishes; clock() fetches the next one at 0
    pub cycles: u8,
    // Cycles since power-on, handy for comparing against nestest-style logs
    pub total_cycles: u64,
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
            .field("total_cycles", &self.total_cycles)
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
            cycles: 0,
            total_cycles: 0,
        }
    }

    // Like powering on a real NES: start wherever the reset vector at $FFFC/$FFFD points
    pub fn reset(&mut self, bus: &Bus) {
        self.pc = u16::from_le_bytes([bus.read(0xFFFC), bus.read(0xFFFD)]) as usize;
        // A real 6502 leaves A/X/Y alone on reset; clearing them (as olcNES does) gives
        // the debugger a clean start
        self.reg_a = 0;
        self.reg_x = 0;
        self.reg_y = 0;
        self.total_cycles = 0;
        self.sp = 0xFD;
        self.sr = 0b0010_0100; // interrupt disable set
        // The reset sequence itself takes 7 cycles before the first instruction runs
        self.cycles = 7;
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

    // The memory the instruction at PC is about to read or write, as (first address, byte
    // count), for the debugger. None for instructions that only touch registers, immediates,
    // and jump/branch targets (those are code, not data). Only peeks at memory, so it's
    // safe to call between steps while the bus has no read side effects.
    pub fn memory_access(&self, bus: &Bus) -> Option<(u16, u16)> {
        let opcode = OPCODES_MAP.get(&bus.read(self.pc as u16))?;
        let sp = STACK_START + self.sp as u16;
        match opcode.mnemonic {
            // JSR pushes the 2-byte return address at SP and SP-1
            "JSR" => Some((sp - 1, 2)),
            // RTS pulls 2 bytes starting just above SP
            "RTS" => Some((sp + 1, 2)),
            "JMP" => None,
            _ => match opcode.mode {
                AddressingMode::NoneAddressing | AddressingMode::Immediate => None,
                ref mode => Some((self.operand_address(mode, bus).0, 1)),
            },
        }
    }

    // Works out the effective address an instruction's operand refers to.
    // For Immediate, that's the operand byte itself (PC + 1).
    // Also returns whether indexing crossed into a new page, which costs read
    // instructions an extra cycle in the Absolute_X/Y and Indirect_Y modes.
    fn operand_address(&self, mode: &AddressingMode, bus: &Bus) -> (u16, bool) {
        let arg = bus.read(self.pc as u16 + 1);
        // Reads a little-endian pointer from page zero; the high byte wraps within page zero
        let zp_word = |ptr: u8| {
            u16::from_le_bytes([bus.read(ptr as u16), bus.read(ptr.wrapping_add(1) as u16)])
        };
        let abs = || u16::from_le_bytes([arg, bus.read(self.pc as u16 + 2)]);

        // Adds an index to a base address and reports whether the high byte changed
        let indexed = |base: u16, index: u8| {
            let addr = base.wrapping_add(index as u16);
            (addr, addr & 0xFF00 != base & 0xFF00)
        };

        match mode {
            AddressingMode::Immediate => (self.pc as u16 + 1, false),
            AddressingMode::ZeroPage => (arg as u16, false),
            // Zero page indexed addresses wrap within page zero
            AddressingMode::ZeroPage_X => (arg.wrapping_add(self.reg_x) as u16, false),
            AddressingMode::ZeroPage_Y => (arg.wrapping_add(self.reg_y) as u16, false),
            AddressingMode::Absolute => (abs(), false),
            AddressingMode::Absolute_X => indexed(abs(), self.reg_x),
            AddressingMode::Absolute_Y => indexed(abs(), self.reg_y),
            // (zp,X): add X to the zero page address first, then read the pointer there
            AddressingMode::Indirect_X => (zp_word(arg.wrapping_add(self.reg_x)), false),
            // (zp),Y: read the pointer from zero page first, then add Y to it
            AddressingMode::Indirect_Y => indexed(zp_word(arg), self.reg_y),
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

    // Returns 1 extra cycle if indexing crossed a page
    fn lda(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (addr, page_crossed) = self.operand_address(mode, bus);
        let value = bus.read(addr);
        self.reg_a = value;
        self.update_zero_and_negative(value);
        page_crossed as u8
    }

    // Returns 1 extra cycle if indexing crossed a page
    fn ldx(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (addr, page_crossed) = self.operand_address(mode, bus);
        let value = bus.read(addr);
        self.reg_x = value;
        self.update_zero_and_negative(value);
        page_crossed as u8
    }

    // Compares A with memory by working out A - M without storing it: C is set if A >= M
    // (unsigned), Z if they're equal, N from bit 7 of the difference.
    // Returns 1 extra cycle if indexing crossed a page
    fn cmp(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (addr, page_crossed) = self.operand_address(mode, bus);
        let value = bus.read(addr);
        let result = self.reg_a.wrapping_sub(value);
        self.set_carry(self.reg_a >= value);
        self.update_zero_and_negative(result);
        page_crossed as u8
    }

    // Like STA, no page-crossing penalty (and STX's modes can't cross a page anyway)
    fn stx(&mut self, mode: &AddressingMode, bus: &mut Bus) {
        let (addr, _) = self.operand_address(mode, bus);
        bus.write(addr, self.reg_x);
    }

    fn txs(&mut self) {
        self.sp = self.reg_x;
    }

    // Unlike TXS, transfers into A/X/Y set Z and N from the copied value
    fn tax(&mut self) {
        self.reg_x = self.reg_a;
        self.update_zero_and_negative(self.reg_x);
    }

    fn tay(&mut self) {
        self.reg_y = self.reg_a;
        self.update_zero_and_negative(self.reg_y);
    }

    // Stores always take the worst-case time already listed in the opcode table,
    // so a page crossing costs nothing extra
    fn sta(&mut self, mode: &AddressingMode, bus: &mut Bus) {
        let (addr, _) = self.operand_address(mode, bus);
        bus.write(addr, self.reg_a);
    }

    fn dex(&mut self) {
        self.reg_x = self.reg_x.wrapping_sub(1);
        self.update_zero_and_negative(self.reg_x);
    }

    // Branch operand is a signed offset from the next instruction (PC + 2).
    // Returns the extra cycles: +1 if taken, +1 more if the target is on a different page
    fn branch(&mut self, condition: bool, bus: &Bus) -> u8 {
        let offset = bus.read(self.pc as u16 + 1) as i8;
        let next = (self.pc as u16).wrapping_add(2);
        let target = next.wrapping_add(offset as u16);

        if !condition {
            return 0;
        }
        self.pc = target as usize;
        if target & 0xFF00 != next & 0xFF00 {
            2
        } else {
            1
        }
    }

    fn jsr(&mut self, operands: &[u8], bus: &mut Bus) {
        // JSR pushes the address of its own last byte (PC + 2), not the next instruction;
        // RTS adds 1 after pulling it

        let return_addr = (self.pc + 2) as u16;
        self.pushw(bus, return_addr);

        // Operand is a little-endian absolute address
        let target = u16::from_le_bytes([operands[0], operands[1]]);
        self.pc = target as usize;
    }

    fn rts(&mut self, bus: &Bus) {
        // JSR pushed the address of its own last byte, so add 1 to land on the next instruction
        let return_addr = self.popw(bus).wrapping_add(1);
        self.pc = return_addr as usize;
    }

    // Advances the CPU by one clock cycle. Like olcNES, the whole instruction runs on its
    // first cycle and the remaining cycles are just counted down, so devices clocked
    // alongside the CPU still see it take the right amount of time.
    pub fn clock(&mut self, bus: &mut Bus) {
        if self.cycles == 0 {
            self.cycles = self.execute(bus);
        }
        self.cycles -= 1;
        self.total_cycles += 1;
    }

    // True when the current instruction has used up all its cycles
    pub fn complete(&self) -> bool {
        self.cycles == 0
    }

    // Runs `steps` whole instructions, clocking through each one's cycles
    pub fn step(&mut self, steps: usize, bus: &mut Bus) {
        for _ in 0..steps {
            // Finish anything already in flight (e.g. the reset sequence) first
            while !self.complete() {
                self.clock(bus);
            }
            loop {
                self.clock(bus);
                if self.complete() {
                    break;
                }
            }
        }
    }

    // Runs one instruction and returns how many cycles it takes: the base count from
    // the opcode table plus any extra the handler reports
    fn execute(&mut self, bus: &mut Bus) -> u8 {
        let code = bus.read(self.pc as u16);
        let opcode = OPCODES_MAP
            .get(&code)
            .unwrap_or_else(|| panic!("OpCode {:x} is not recognized", code));
        let pc_before = self.pc;

        let extra_cycles = match opcode.mnemonic {
            "CLD" => {
                self.set_decimal(false);
                0
            }
            "CMP" => self.cmp(&opcode.mode, bus),
            "DEX" => {
                self.dex();
                0
            }
            "SEI" => {
                self.set_interrupt_disable(true);
                0
            }
            "JSR" => {
                let operands: Vec<u8> = (1..opcode.len as u16)
                    .map(|i| bus.read(self.pc as u16 + i))
                    .collect();
                self.jsr(&operands, bus);
                0
            }
            "RTS" => {
                self.rts(bus);
                0
            }
            "LDX" => self.ldx(&opcode.mode, bus),
            "LDA" => self.lda(&opcode.mode, bus),
            "NOP" => 0,
            "STA" => {
                self.sta(&opcode.mode, bus);
                0
            }
            "STX" => {
                self.stx(&opcode.mode, bus);
                0
            }
            "TAX" => {
                self.tax();
                0
            }
            "TAY" => {
                self.tay();
                0
            }
            "TXS" => {
                self.txs();
                0
            }
            "BPL" => self.branch(self.sr & FLAG_NEGATIVE == 0, bus),
            _ => {
                eprintln!("no mneumic");
                panic!("no mneumic");
            }
        };
        // Only advance if the instruction didn't jump
        if self.pc == pc_before {
            self.pc += opcode.len as usize;
        }
        opcode.cycles + extra_cycles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Loads `program` at $0600, runs one instruction and returns the cycles it took
    fn cycles_for(program: &[u8], setup: impl FnOnce(&mut CPU)) -> u64 {
        let mut bus = Bus::new();
        bus.load(0x0600, program);
        let mut cpu = CPU::new();
        setup(&mut cpu);
        cpu.step(1, &mut bus);
        cpu.total_cycles
    }

    #[test]
    fn lda_absolute_x_same_page() {
        // LDA $1000,X with X = 1 stays on page $10
        assert_eq!(cycles_for(&[0xbd, 0x00, 0x10], |c| c.reg_x = 1), 4);
    }

    #[test]
    fn lda_absolute_x_page_crossed() {
        // LDA $10FF,X with X = 1 lands on $1100
        assert_eq!(cycles_for(&[0xbd, 0xff, 0x10], |c| c.reg_x = 1), 5);
    }

    #[test]
    fn sta_absolute_x_page_crossed_has_no_penalty() {
        assert_eq!(cycles_for(&[0x9d, 0xff, 0x10], |c| c.reg_x = 1), 5);
    }

    #[test]
    fn stx_writes_x_to_each_mode() {
        // (program, X, Y, expected address, expected cycles)
        let cases: [(&[u8], u8, u8, u16, u64); 4] = [
            (&[0x86, 0x10], 0x42, 0, 0x0010, 3),       // STX $10
            (&[0x96, 0x10], 0x42, 0x05, 0x0015, 4),    // STX $10,Y
            (&[0x96, 0xff], 0x42, 0x02, 0x0001, 4),    // STX $FF,Y wraps within page zero
            (&[0x8e, 0x00, 0x20], 0x42, 0, 0x2000, 4), // STX $2000
        ];
        for (program, x, y, addr, cycles) in cases {
            let mut bus = Bus::new();
            bus.load(0x0600, program);
            let mut cpu = CPU::new();
            cpu.reg_x = x;
            cpu.reg_y = y;
            cpu.step(1, &mut bus);
            assert_eq!(bus.read(addr), x, "program {:02X?}", program);
            assert_eq!(cpu.total_cycles, cycles, "program {:02X?}", program);
            assert_eq!(cpu.pc, 0x0600 + program.len(), "program {:02X?}", program);
        }
    }

    #[test]
    fn memory_access_reports_what_the_next_instruction_touches() {
        let access = |program: &[u8]| {
            let mut bus = Bus::new();
            bus.load(0x0600, program);
            let mut cpu = CPU::new();
            cpu.sp = 0xFD;
            cpu.reg_x = 2;
            cpu.memory_access(&bus)
        };
        assert_eq!(access(&[0xad, 0x02, 0x20]), Some((0x2002, 1))); // LDA $2002
        assert_eq!(access(&[0x95, 0x10]), Some((0x0012, 1))); // STA $10,X
        assert_eq!(access(&[0x20, 0x00, 0x80]), Some((0x01FC, 2))); // JSR pushes at SP, SP-1
        assert_eq!(access(&[0x60]), Some((0x01FE, 2))); // RTS pulls from SP+1, SP+2
        assert_eq!(access(&[0xa9, 0x01]), None); // LDA #$01
        assert_eq!(access(&[0xca]), None); // DEX
        assert_eq!(access(&[0x10, 0x02]), None); // BPL
    }

    #[test]
    fn cmp_sets_carry_zero_and_negative() {
        // (A, M, expected C, Z, N)
        let cases = [
            (0x05, 0x03, true, false, false), // A > M
            (0x05, 0x05, true, true, false),  // A == M
            (0x01, 0x02, false, false, true), // A < M, difference $FF
            (0x00, 0x80, false, false, true), // difference $80
            (0x80, 0x00, true, false, true),  // A > M but difference has bit 7 set
            (0xFF, 0x01, true, false, true),  // unsigned: $FF is greater than $01
        ];
        for (a, m, c, z, n) in cases {
            let mut bus = Bus::new();
            bus.load(0x0600, &[0xc9, m]); // CMP #m
            let mut cpu = CPU::new();
            cpu.reg_a = a;
            // Start with every flag set the opposite way, so stale flags would show up
            cpu.set_carry(!c);
            cpu.set_zero(!z);
            cpu.set_negative(!n);
            cpu.step(1, &mut bus);
            let ctx = format!("CMP A=${:02X} M=${:02X}", a, m);
            assert_eq!(cpu.sr & FLAG_CARRY != 0, c, "C after {}", ctx);
            assert_eq!(cpu.sr & FLAG_ZERO != 0, z, "Z after {}", ctx);
            assert_eq!(cpu.sr & FLAG_NEGATIVE != 0, n, "N after {}", ctx);
            assert_eq!(cpu.reg_a, a, "A must be unchanged after {}", ctx);
        }
    }

    #[test]
    fn cmp_absolute_x_page_crossed() {
        // CMP $10FF,X with X = 1 reads $1100: 4 + 1 cycles
        assert_eq!(cycles_for(&[0xdd, 0xff, 0x10], |c| c.reg_x = 1), 5);
    }

    #[test]
    fn tax_and_tay_copy_a_and_set_flags() {
        // (opcode, A, expected Z, N)
        let cases = [
            (0xaa, 0x42, false, false), // TAX
            (0xaa, 0x00, true, false),
            (0xaa, 0x80, false, true),
            (0xa8, 0x42, false, false), // TAY
            (0xa8, 0x00, true, false),
            (0xa8, 0x80, false, true),
        ];
        for (code, a, z, n) in cases {
            let mut bus = Bus::new();
            bus.load(0x0600, &[code]);
            let mut cpu = CPU::new();
            cpu.reg_a = a;
            // Opposite flags first, so a missing flag update fails the test
            cpu.set_zero(!z);
            cpu.set_negative(!n);
            cpu.step(1, &mut bus);
            let ctx = format!("opcode ${:02X} with A=${:02X}", code, a);
            let target = if code == 0xaa { cpu.reg_x } else { cpu.reg_y };
            assert_eq!(target, a, "{}", ctx);
            assert_eq!(cpu.reg_a, a, "A must be unchanged after {}", ctx);
            assert_eq!(cpu.sr & FLAG_ZERO != 0, z, "Z after {}", ctx);
            assert_eq!(cpu.sr & FLAG_NEGATIVE != 0, n, "N after {}", ctx);
            assert_eq!(cpu.total_cycles, 2, "cycles for {}", ctx);
            assert_eq!(cpu.pc, 0x0601, "PC after {}", ctx);
        }
    }

    #[test]
    fn bpl_not_taken() {
        assert_eq!(cycles_for(&[0x10, 0x02], |c| c.set_negative(true)), 2);
    }

    #[test]
    fn bpl_taken_same_page() {
        assert_eq!(cycles_for(&[0x10, 0x02], |_| {}), 3);
    }

    #[test]
    fn bpl_taken_page_crossed() {
        // From $0602, branching back 3 lands on $05FF
        assert_eq!(cycles_for(&[0x10, 0xfd], |_| {}), 4);
    }

    #[test]
    fn reset_cycles_are_not_counted_as_a_step() {
        let mut bus = Bus::new();
        bus.load(0xFFFC, &[0x00, 0x06]);
        bus.load(0x0600, &[0xd8]); // CLD
        let mut cpu = CPU::new();
        cpu.reset(&bus);
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x0601);
        assert_eq!(cpu.total_cycles, 7 + 2);
    }
}
