use crate::bus::Bus;
use crate::disasm;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};

use crate::opcode::{OPCODES_MAP, OpCode};

// Mnemonics that CPU::execute has a handler for. Keep in sync with the match in execute
// (All 56 official 6502 instructions)
pub const IMPLEMENTED: &[&str] = &[
    "ADC", "AND", "ASL", "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRK", "BVC", "BVS",
    "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY", "DEC", "DEX", "DEY", "EOR", "INC", "INX",
    "INY", "JMP", "JSR", "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PLA", "PLP",
    "ROL", "ROR", "RTI", "RTS", "SBC", "SEC", "SED", "SEI", "STA", "STX", "STY", "TAX", "TAY",
    "TSX", "TXA", "TXS", "TYA",
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
    // ASL A, LSR A, ROL A, ROR A: operate on the accumulator instead of memory
    Accumulator,
    // JMP ($xxxx) only: the operand points at the 2-byte target address
    Indirect,
    // Branches: a signed offset from the next instruction. The target is code, so
    // `branch` works it out itself rather than going through operand_address
    Relative,
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
    // LOGMODE: when set, every instruction (and interrupt) is written here. See enable_log
    log: Option<Box<dyn Write>>,
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
            log: None,
        }
    }

    // LOGMODE (like olcNES's #ifdef LOGMODE): writes one line per instruction to `path`, e.g.
    //          7 PC:EDB7 D8        CLD          A:00 X:00 Y:00 ..U..I.. STKP:FD
    // Each line is the state *before* the instruction runs (the nestest log convention),
    // with the cycle count at its start. Slow, so only turn it on when debugging
    pub fn enable_log(&mut self, path: &str) -> io::Result<()> {
        self.log = Some(Box::new(BufWriter::new(File::create(path)?)));
        Ok(())
    }

    // Flags in olc's log style: a letter when set, '.' when clear, NVUBDIZC
    fn flags_string(&self) -> String {
        "NVUBDIZC"
            .chars()
            .enumerate()
            .map(|(i, c)| if self.sr & (0x80 >> i) != 0 { c } else { '.' })
            .collect()
    }

    fn log_line(&mut self, line: String) {
        if let Some(log) = self.log.as_mut() {
            // Losing a log line isn't worth stopping the emulator for
            let _ = writeln!(log, "{}", line);
        }
    }

    fn log_instruction(&mut self, bus: &Bus) {
        if self.log.is_none() {
            return;
        }
        let ins = disasm::decode(bus, self.pc as u16);
        let line = format!(
            "{:>10} PC:{:04X} {:<8}  {:<13} A:{:02X} X:{:02X} Y:{:02X} {} STKP:{:02X}",
            self.total_cycles,
            ins.addr,
            ins.bytes,
            ins.text,
            self.reg_a,
            self.reg_x,
            self.reg_y,
            self.flags_string(),
            self.sp
        );
        self.log_line(line);
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
            // Single-byte pushes write at SP; pulls read just above it
            "PHA" | "PHP" => Some((sp, 1)),
            "PLA" | "PLP" => Some((sp + 1, 1)),
            // BRK pushes PC (2 bytes) and status; RTI pulls status and PC
            "BRK" => Some((sp - 2, 3)),
            "RTI" => Some((sp + 1, 3)),
            // JMP ($xxxx) reads its target from a 2-byte pointer; JMP $xxxx touches no data
            "JMP" => match opcode.mode {
                AddressingMode::Indirect => Some((
                    u16::from_le_bytes([
                        bus.read(self.pc as u16 + 1),
                        bus.read(self.pc as u16 + 2),
                    ]),
                    2,
                )),
                _ => None,
            },
            _ => match opcode.mode {
                AddressingMode::NoneAddressing
                | AddressingMode::Immediate
                | AddressingMode::Accumulator
                | AddressingMode::Relative => None,
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
            // The 6502 never carries into the pointer's high byte: JMP ($10FF) reads the
            // low byte from $10FF but the high byte from $1000, not $1100
            AddressingMode::Indirect => {
                let ptr = abs();
                let hi_ptr = (ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF);
                (u16::from_le_bytes([bus.read(ptr), bus.read(hi_ptr)]), false)
            }
            AddressingMode::Relative
            | AddressingMode::Accumulator
            | AddressingMode::NoneAddressing => {
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

    // Reads the operand's value. Returns it with 1 extra cycle if indexing crossed a page
    fn read_operand(&self, mode: &AddressingMode, bus: &Bus) -> (u8, u8) {
        let (addr, page_crossed) = self.operand_address(mode, bus);
        (bus.read(addr), page_crossed as u8)
    }

    // CMP/CPX/CPY: compares a register with memory by working out reg - M without storing
    // it: C is set if reg >= M (unsigned), Z if they're equal, N from bit 7 of the difference.
    // Returns 1 extra cycle if indexing crossed a page
    fn compare(&mut self, reg: u8, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.set_carry(reg >= value);
        self.update_zero_and_negative(reg.wrapping_sub(value));
        extra
    }

    fn cmp(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        self.compare(self.reg_a, mode, bus)
    }

    fn ldy(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.reg_y = value;
        self.update_zero_and_negative(value);
        extra
    }

    fn sty(&mut self, mode: &AddressingMode, bus: &mut Bus) {
        let (addr, _) = self.operand_address(mode, bus);
        bus.write(addr, self.reg_y);
    }

    // A = A + M + C. The NES's 6502 has no decimal mode, so D is ignored.
    // C is set on unsigned overflow (result > $FF). V is set on signed overflow: both inputs
    // have the same sign but the result's sign differs, e.g. $50 + $50 = $A0 (80 + 80 = -96)
    fn add_with_carry(&mut self, value: u8) {
        let a = self.reg_a;
        let sum = a as u16 + value as u16 + (self.sr & FLAG_CARRY) as u16;
        let result = sum as u8;
        self.set_carry(sum > 0xFF);
        self.set_overflow((a ^ result) & (value ^ result) & 0x80 != 0);
        self.reg_a = result;
        self.update_zero_and_negative(result);
    }

    fn adc(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.add_with_carry(value);
        extra
    }

    // A = A - M - (1 - C). In two's complement that's A + !M + C, so SBC is ADC with the
    // operand inverted. C ends up as "no borrow": set if A >= M (with C set going in)
    fn sbc(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.add_with_carry(!value);
        extra
    }

    fn and(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.reg_a &= value;
        self.update_zero_and_negative(self.reg_a);
        extra
    }

    fn ora(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.reg_a |= value;
        self.update_zero_and_negative(self.reg_a);
        extra
    }

    fn eor(&mut self, mode: &AddressingMode, bus: &Bus) -> u8 {
        let (value, extra) = self.read_operand(mode, bus);
        self.reg_a ^= value;
        self.update_zero_and_negative(self.reg_a);
        extra
    }

    // Z from A & M (A is unchanged); N and V are copied straight from bits 7 and 6 of M
    fn bit(&mut self, mode: &AddressingMode, bus: &Bus) {
        let (value, _) = self.read_operand(mode, bus);
        self.set_zero(self.reg_a & value == 0);
        self.set_negative(value & 0x80 != 0);
        self.set_overflow(value & 0x40 != 0);
    }

    // Read-modify-write: applies `op` to A (Accumulator mode) or to the byte in memory,
    // writing the result back. These take the worst-case time from the table, so indexed
    // forms never get a page-crossing cycle
    fn read_modify_write(
        &mut self,
        mode: &AddressingMode,
        bus: &mut Bus,
        op: fn(&mut CPU, u8) -> u8,
    ) {
        if let AddressingMode::Accumulator = mode {
            let a = self.reg_a;
            self.reg_a = op(self, a);
        } else {
            let (addr, _) = self.operand_address(mode, bus);
            let value = bus.read(addr);
            let result = op(self, value);
            bus.write(addr, result);
        }
    }

    // The operations for read_modify_write. Each sets Z/N from its result

    // Shift left: bit 7 goes into C, 0 comes in at bit 0
    fn shift_left(&mut self, value: u8) -> u8 {
        self.set_carry(value & 0x80 != 0);
        let result = value << 1;
        self.update_zero_and_negative(result);
        result
    }

    // Shift right: bit 0 goes into C, 0 comes in at bit 7 (so N always ends up clear)
    fn shift_right(&mut self, value: u8) -> u8 {
        self.set_carry(value & 0x01 != 0);
        let result = value >> 1;
        self.update_zero_and_negative(result);
        result
    }

    // Rotate left through carry: old C comes in at bit 0, bit 7 goes into C
    fn rotate_left(&mut self, value: u8) -> u8 {
        let carry_in = self.sr & FLAG_CARRY;
        self.set_carry(value & 0x80 != 0);
        let result = (value << 1) | carry_in;
        self.update_zero_and_negative(result);
        result
    }

    // Rotate right through carry: old C comes in at bit 7, bit 0 goes into C
    fn rotate_right(&mut self, value: u8) -> u8 {
        let carry_in = self.sr & FLAG_CARRY;
        self.set_carry(value & 0x01 != 0);
        let result = (value >> 1) | (carry_in << 7);
        self.update_zero_and_negative(result);
        result
    }

    fn increment(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.update_zero_and_negative(result);
        result
    }

    fn decrement(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.update_zero_and_negative(result);
        result
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

    fn txa(&mut self) {
        self.reg_a = self.reg_x;
        self.update_zero_and_negative(self.reg_a);
    }

    fn tya(&mut self) {
        self.reg_a = self.reg_y;
        self.update_zero_and_negative(self.reg_a);
    }

    // Wraps $FF -> $00 (setting Z), the mirror of DEX
    fn inx(&mut self) {
        self.reg_x = self.reg_x.wrapping_add(1);
        self.update_zero_and_negative(self.reg_x);
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

    fn iny(&mut self) {
        self.reg_y = self.reg_y.wrapping_add(1);
        self.update_zero_and_negative(self.reg_y);
    }

    fn dey(&mut self) {
        self.reg_y = self.reg_y.wrapping_sub(1);
        self.update_zero_and_negative(self.reg_y);
    }

    // Unlike TXS, TSX sets Z and N like the other transfers
    fn tsx(&mut self) {
        self.reg_x = self.sp;
        self.update_zero_and_negative(self.reg_x);
    }

    // Branch operand is a signed offset from the next instruction (PC + 2).
    // Returns the extra cycles: +1 if taken, +1 more if the target is on a different page
    fn branch(&mut self, condition: bool, bus: &Bus) -> u8 {
        let offset = bus.read(self.pc as u16 + 1) as i8;
        let next = (self.pc as u16).wrapping_add(2);
        let target = next.wrapping_add(offset as u16);

        if !condition {
            self.pc = next as usize;
            return 0;
        }
        self.pc = target as usize;
        if target & 0xFF00 != next & 0xFF00 {
            2
        } else {
            1
        }
    }

    // Absolute: jump to the operand. Indirect: jump to the address stored at the operand
    fn jmp(&mut self, mode: &AddressingMode, bus: &Bus) {
        let (target, _) = self.operand_address(mode, bus);
        println!("jmp {:?} to {:#06x}", mode, target);
        self.pc = target as usize;
    }

    fn pha(&mut self, bus: &mut Bus) {
        self.pushb(bus, self.reg_a);
    }

    // The B flag only exists in the pushed copy: PHP pushes the status with B and the
    // unused bit 5 both set
    fn php(&mut self, bus: &mut Bus) {
        self.pushb(bus, self.sr | FLAG_BREAK | FLAG_UNUSED);
    }

    fn pla(&mut self, bus: &Bus) {
        self.reg_a = self.popb(bus);
        self.update_zero_and_negative(self.reg_a);
    }

    // Restores every flag from the stack except B, which isn't a real register bit, and
    // bit 5, which always reads as 1
    fn plp(&mut self, bus: &Bus) {
        let value = self.popb(bus);
        self.sr = (value & !FLAG_BREAK) | FLAG_UNUSED;
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

    // Software interrupt: pushes PC + 2 (BRK skips a padding byte after it) and the status
    // with B set, disables interrupts, then jumps through the IRQ/BRK vector at $FFFE
    fn brk(&mut self, bus: &mut Bus) {
        self.pushw(bus, (self.pc as u16).wrapping_add(2));
        self.pushb(bus, self.sr | FLAG_BREAK | FLAG_UNUSED);
        self.set_interrupt_disable(true);
        self.pc = u16::from_le_bytes([bus.read(0xFFFE), bus.read(0xFFFF)]) as usize;
    }

    // Hardware interrupts: push PC and the status (B clear, bit 5 set) *then* set I, and jump
    // through `vector`. The pushed status keeps the old I, so RTI re-enables interrupts.
    // (olcNES sets I before pushing; real hardware doesn't.) Takes 7 cycles. Because
    // execute() runs a whole instruction on its first cycle, PC is already at the next
    // instruction, so calling this mid-countdown still returns to the right place
    fn interrupt(&mut self, bus: &mut Bus, vector: u16, name: &str) {
        self.pushw(bus, self.pc as u16);
        self.pushb(bus, (self.sr & !FLAG_BREAK) | FLAG_UNUSED);
        self.set_interrupt_disable(true);
        let from = self.pc;
        self.pc = u16::from_le_bytes([bus.read(vector), bus.read(vector + 1)]) as usize;
        self.cycles += 7;
        let line = format!(
            "{:>10} ---- {} from {:04X} to {:04X}",
            self.total_cycles, name, from, self.pc
        );
        self.log_line(line);
    }

    // Interrupt request: ignored while the I flag is set (SEI). Returns whether it was taken.
    // On the NES this comes from the APU and some mappers
    pub fn irq(&mut self, bus: &mut Bus) -> bool {
        if self.sr & FLAG_INTERRUPT_DISABLE != 0 {
            return false;
        }
        self.interrupt(bus, 0xFFFE, "IRQ");
        true
    }

    // Non-maskable interrupt: always taken. On the NES the PPU fires it at the start of
    // every vblank, which is when games do their per-frame work
    pub fn nmi(&mut self, bus: &mut Bus) {
        self.interrupt(bus, 0xFFFA, "NMI");
    }

    // Return from interrupt: pulls the status (like PLP) then PC. Unlike RTS there's no +1,
    // because interrupts push the actual address to return to
    fn rti(&mut self, bus: &Bus) {
        self.plp(bus);
        self.pc = self.popw(bus) as usize;
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
        self.log_instruction(bus);

        let mode = &opcode.mode;
        let extra_cycles = match opcode.mnemonic {
            // Loads and stores
            "LDA" => self.lda(mode, bus),
            "LDX" => self.ldx(mode, bus),
            "LDY" => self.ldy(mode, bus),
            "STA" => {
                self.sta(mode, bus);
                0
            }
            "STX" => {
                self.stx(mode, bus);
                0
            }
            "STY" => {
                self.sty(mode, bus);
                0
            }

            // Arithmetic, logic and compares
            "ADC" => self.adc(mode, bus),
            "SBC" => self.sbc(mode, bus),
            "AND" => self.and(mode, bus),
            "ORA" => self.ora(mode, bus),
            "EOR" => self.eor(mode, bus),
            "CMP" => self.cmp(mode, bus),
            "CPX" => self.compare(self.reg_x, mode, bus),
            "CPY" => self.compare(self.reg_y, mode, bus),
            "BIT" => {
                self.bit(mode, bus);
                0
            }

            // Read-modify-write: shifts, rotates, memory increment/decrement
            "ASL" => {
                self.read_modify_write(mode, bus, CPU::shift_left);
                0
            }
            "LSR" => {
                self.read_modify_write(mode, bus, CPU::shift_right);
                0
            }
            "ROL" => {
                self.read_modify_write(mode, bus, CPU::rotate_left);
                0
            }
            "ROR" => {
                self.read_modify_write(mode, bus, CPU::rotate_right);
                0
            }
            "INC" => {
                self.read_modify_write(mode, bus, CPU::increment);
                0
            }
            "DEC" => {
                self.read_modify_write(mode, bus, CPU::decrement);
                0
            }

            // Register increments, decrements and transfers
            "INX" => {
                self.inx();
                0
            }
            "INY" => {
                self.iny();
                0
            }
            "DEX" => {
                self.dex();
                0
            }
            "DEY" => {
                self.dey();
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
            "TXA" => {
                self.txa();
                0
            }
            "TYA" => {
                self.tya();
                0
            }
            "TSX" => {
                self.tsx();
                0
            }
            "TXS" => {
                self.txs();
                0
            }

            // Stack
            "PHA" => {
                self.pha(bus);
                0
            }
            "PHP" => {
                self.php(bus);
                0
            }
            "PLA" => {
                self.pla(bus);
                0
            }
            "PLP" => {
                self.plp(bus);
                0
            }

            // Flags
            "CLC" => {
                self.set_carry(false);
                0
            }
            "SEC" => {
                self.set_carry(true);
                0
            }
            "CLI" => {
                self.set_interrupt_disable(false);
                0
            }
            "SEI" => {
                self.set_interrupt_disable(true);
                0
            }
            "CLV" => {
                self.set_overflow(false);
                0
            }
            "CLD" => {
                self.set_decimal(false);
                0
            }
            "SED" => {
                self.set_decimal(true);
                0
            }

            // Branches: each tests one flag
            "BCC" => self.branch(self.sr & FLAG_CARRY == 0, bus),
            "BCS" => self.branch(self.sr & FLAG_CARRY != 0, bus),
            "BNE" => self.branch(self.sr & FLAG_ZERO == 0, bus),
            "BEQ" => self.branch(self.sr & FLAG_ZERO != 0, bus),
            "BPL" => self.branch(self.sr & FLAG_NEGATIVE == 0, bus),
            "BMI" => self.branch(self.sr & FLAG_NEGATIVE != 0, bus),
            "BVC" => self.branch(self.sr & FLAG_OVERFLOW == 0, bus),
            "BVS" => self.branch(self.sr & FLAG_OVERFLOW != 0, bus),

            // Jumps, subroutines and interrupts
            "JMP" => {
                self.jmp(mode, bus);
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
            "BRK" => {
                self.brk(bus);
                0
            }
            "RTI" => {
                self.rti(bus);
                0
            }

            "NOP" => 0,
            _ => panic!("no handler for {}", opcode.mnemonic),
        };
        // Control-flow instructions always set PC themselves (branches set it to the next
        // instruction when not taken), so a jump to its own address, like JMP *, works.
        // Everything else moves on to the next instruction
        let sets_pc = matches!(opcode.mode, AddressingMode::Relative)
            || matches!(opcode.mnemonic, "JMP" | "JSR" | "RTS" | "BRK" | "RTI");
        if !sets_pc {
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
    fn bne_not_taken_when_zero_set() {
        assert_eq!(cycles_for(&[0xd0, 0x02], |c| c.set_zero(true)), 2);
    }

    #[test]
    fn bne_taken_when_zero_clear() {
        let mut bus = Bus::new();
        bus.load(0x0600, &[0xd0, 0x02]); // BNE +2 -> $0604
        let mut cpu = CPU::new();
        cpu.set_zero(false);
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x0604);
        assert_eq!(cpu.total_cycles, 3);
    }

    #[test]
    fn bne_taken_page_crossed() {
        // From $0602, branching back 3 lands on $05FF
        assert_eq!(cycles_for(&[0xd0, 0xfd], |c| c.set_zero(false)), 4);
    }

    #[test]
    fn cmp_then_bne_skips_when_not_equal() {
        // LDA #$00; CMP #$4B; BNE +2 -- like Donkey Kong's check at $EDD2
        let mut bus = Bus::new();
        bus.load(0x0600, &[0xa9, 0x00, 0xc9, 0x4b, 0xd0, 0x02]);
        let mut cpu = CPU::new();
        cpu.step(3, &mut bus);
        assert_eq!(cpu.pc, 0x0608);
    }

    #[test]
    fn txa_copies_x_and_sets_flags() {
        // (X, expected Z, N)
        for (x, z, n) in [
            (0x42, false, false),
            (0x00, true, false),
            (0x80, false, true),
        ] {
            let mut bus = Bus::new();
            bus.load(0x0600, &[0x8a]); // TXA
            let mut cpu = CPU::new();
            cpu.reg_a = 0x11;
            cpu.reg_x = x;
            cpu.set_zero(!z);
            cpu.set_negative(!n);
            cpu.step(1, &mut bus);
            assert_eq!(cpu.reg_a, x, "A after TXA with X=${:02X}", x);
            assert_eq!(cpu.reg_x, x, "X must be unchanged");
            assert_eq!(cpu.sr & FLAG_ZERO != 0, z, "Z after TXA with X=${:02X}", x);
            assert_eq!(
                cpu.sr & FLAG_NEGATIVE != 0,
                n,
                "N after TXA with X=${:02X}",
                x
            );
            assert_eq!(cpu.total_cycles, 2);
        }
    }

    #[test]
    fn tya_copies_y_and_sets_flags() {
        // (Y, expected Z, N)
        for (y, z, n) in [
            (0x42, false, false),
            (0x00, true, false),
            (0x80, false, true),
        ] {
            let mut bus = Bus::new();
            bus.load(0x0600, &[0x98]); // TYA
            let mut cpu = CPU::new();
            cpu.reg_a = 0x11;
            cpu.reg_y = y;
            cpu.set_zero(!z);
            cpu.set_negative(!n);
            cpu.step(1, &mut bus);
            assert_eq!(cpu.reg_a, y, "A after TYA with Y=${:02X}", y);
            assert_eq!(cpu.reg_y, y, "Y must be unchanged");
            assert_eq!(cpu.sr & FLAG_ZERO != 0, z, "Z after TYA with Y=${:02X}", y);
            assert_eq!(
                cpu.sr & FLAG_NEGATIVE != 0,
                n,
                "N after TYA with Y=${:02X}",
                y
            );
            assert_eq!(cpu.total_cycles, 2);
        }
    }

    #[test]
    fn inx_increments_wraps_and_sets_flags() {
        // (X before, X after, expected Z, N)
        let cases = [
            (0x04, 0x05, false, false),
            (0x7F, 0x80, false, true),
            (0xFF, 0x00, true, false), // wraps
        ];
        for (before, after, z, n) in cases {
            let mut bus = Bus::new();
            bus.load(0x0600, &[0xe8]); // INX
            let mut cpu = CPU::new();
            cpu.reg_x = before;
            cpu.set_zero(!z);
            cpu.set_negative(!n);
            cpu.step(1, &mut bus);
            assert_eq!(cpu.reg_x, after, "X after INX from ${:02X}", before);
            assert_eq!(
                cpu.sr & FLAG_ZERO != 0,
                z,
                "Z after INX from ${:02X}",
                before
            );
            assert_eq!(
                cpu.sr & FLAG_NEGATIVE != 0,
                n,
                "N after INX from ${:02X}",
                before
            );
            assert_eq!(cpu.total_cycles, 2);
        }
    }

    #[test]
    fn jmp_absolute() {
        let mut bus = Bus::new();
        bus.load(0x0600, &[0x4c, 0x34, 0x12]); // JMP $1234
        let mut cpu = CPU::new();
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x1234);
        assert_eq!(cpu.total_cycles, 3);
    }

    #[test]
    fn jmp_to_itself_stays_put() {
        // JMP $0600 at $0600: the classic "wait forever" loop
        let mut bus = Bus::new();
        bus.load(0x0600, &[0x4c, 0x00, 0x06]);
        let mut cpu = CPU::new();
        cpu.step(3, &mut bus);
        assert_eq!(cpu.pc, 0x0600);
        assert_eq!(cpu.total_cycles, 9);
    }

    #[test]
    fn jmp_indirect() {
        let mut bus = Bus::new();
        bus.load(0x0600, &[0x6c, 0x20, 0x01]); // JMP ($0120)
        bus.load(0x0120, &[0x78, 0x56]);
        let mut cpu = CPU::new();
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x5678);
        assert_eq!(cpu.total_cycles, 5);
    }

    #[test]
    fn jmp_indirect_page_wrap_bug() {
        // JMP ($02FF) takes the high byte from $0200, not $0300
        let mut bus = Bus::new();
        bus.load(0x0600, &[0x6c, 0xff, 0x02]);
        bus.write(0x02FF, 0x34);
        bus.write(0x0200, 0x12);
        bus.write(0x0300, 0x99);
        let mut cpu = CPU::new();
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x1234);
    }

    #[test]
    fn branch_to_itself_stays_put() {
        // BNE -2 at $0600 with Z clear branches back to itself
        let mut bus = Bus::new();
        bus.load(0x0600, &[0xd0, 0xfe]);
        let mut cpu = CPU::new();
        cpu.step(2, &mut bus);
        assert_eq!(cpu.pc, 0x0600);
    }

    // Runs `program` from $0600 with SP = $FD and returns the CPU after `steps` instructions
    fn run(program: &[u8], steps: usize, setup: impl FnOnce(&mut CPU, &mut Bus)) -> (CPU, Bus) {
        let mut bus = Bus::new();
        bus.load(0x0600, program);
        let mut cpu = CPU::new();
        cpu.sp = 0xFD;
        setup(&mut cpu, &mut bus);
        cpu.step(steps, &mut bus);
        (cpu, bus)
    }

    #[test]
    fn pha_pushes_a() {
        let (cpu, bus) = run(&[0x48], 1, |c, _| c.reg_a = 0x42);
        assert_eq!(bus.read(0x01FD), 0x42);
        assert_eq!(cpu.sp, 0xFC);
        assert_eq!(cpu.total_cycles, 3);
    }

    #[test]
    fn php_pushes_status_with_b_and_bit5_set() {
        let (cpu, bus) = run(&[0x08], 1, |c, _| c.sr = FLAG_CARRY | FLAG_NEGATIVE);
        assert_eq!(
            bus.read(0x01FD),
            FLAG_CARRY | FLAG_NEGATIVE | FLAG_BREAK | FLAG_UNUSED
        );
        // The status register itself is unchanged
        assert_eq!(cpu.sr, FLAG_CARRY | FLAG_NEGATIVE);
        assert_eq!(cpu.sp, 0xFC);
        assert_eq!(cpu.total_cycles, 3);
    }

    #[test]
    fn pla_pulls_a_and_sets_flags() {
        // (pulled value, expected Z, N)
        for (value, z, n) in [
            (0x42, false, false),
            (0x00, true, false),
            (0x80, false, true),
        ] {
            let (cpu, _) = run(&[0x68], 1, |c, b| {
                c.sp = 0xFC;
                b.write(0x01FD, value);
                c.set_zero(!z);
                c.set_negative(!n);
            });
            assert_eq!(cpu.reg_a, value, "A after PLA of ${:02X}", value);
            assert_eq!(cpu.sr & FLAG_ZERO != 0, z, "Z after PLA of ${:02X}", value);
            assert_eq!(
                cpu.sr & FLAG_NEGATIVE != 0,
                n,
                "N after PLA of ${:02X}",
                value
            );
            assert_eq!(cpu.sp, 0xFD);
            assert_eq!(cpu.total_cycles, 4);
        }
    }

    #[test]
    fn plp_restores_flags_ignoring_b() {
        let (cpu, _) = run(&[0x28], 1, |c, b| {
            c.sp = 0xFC;
            b.write(0x01FD, 0xFF); // every bit set, including B
        });
        assert_eq!(cpu.sr, 0xFF & !FLAG_BREAK);
        assert_eq!(cpu.sp, 0xFD);
        assert_eq!(cpu.total_cycles, 4);

        // Bit 5 reads as 1 even if the pulled byte has it clear
        let (cpu, _) = run(&[0x28], 1, |c, b| {
            c.sp = 0xFC;
            b.write(0x01FD, 0x00);
        });
        assert_eq!(cpu.sr, FLAG_UNUSED);
    }

    #[test]
    fn push_then_pull_round_trips() {
        // LDA #$37; PHA; LDA #$00; PLA
        let (cpu, _) = run(&[0xa9, 0x37, 0x48, 0xa9, 0x00, 0x68], 4, |_, _| {});
        assert_eq!(cpu.reg_a, 0x37);
        assert_eq!(cpu.sp, 0xFD);

        // PHP; PLP with C and Z set: status comes back the same
        let (cpu, _) = run(&[0x08, 0x28], 2, |c, _| {
            c.sr = FLAG_CARRY | FLAG_ZERO | FLAG_UNUSED
        });
        assert_eq!(cpu.sr, FLAG_CARRY | FLAG_ZERO | FLAG_UNUSED);
    }

    #[test]
    fn memory_access_for_stack_pushes_and_pulls() {
        for (code, addr) in [
            (0x48, 0x01FD),
            (0x08, 0x01FD),
            (0x68, 0x01FE),
            (0x28, 0x01FE),
        ] {
            let mut bus = Bus::new();
            bus.load(0x0600, &[code]);
            let mut cpu = CPU::new();
            cpu.sp = 0xFD;
            assert_eq!(
                cpu.memory_access(&bus),
                Some((addr, 1)),
                "opcode ${:02X}",
                code
            );
        }
    }

    // Flags as a compact string like "NV..ZC" for readable assertions (only N V Z C)
    fn nvzc(cpu: &CPU) -> String {
        [
            (FLAG_NEGATIVE, 'N'),
            (FLAG_OVERFLOW, 'V'),
            (FLAG_ZERO, 'Z'),
            (FLAG_CARRY, 'C'),
        ]
        .iter()
        .map(|&(f, c)| if cpu.sr & f != 0 { c } else { '.' })
        .collect()
    }

    #[test]
    fn adc_carry_and_overflow() {
        // (A, M, carry in, expected A, flags NVZC)
        let cases = [
            (0x01, 0x01, false, 0x02, "...."),
            (0x01, 0x01, true, 0x03, "...."),  // carry in adds 1
            (0xFF, 0x01, false, 0x00, "..ZC"), // unsigned overflow sets C
            (0x50, 0x50, false, 0xA0, "NV.."), // 80 + 80: signed overflow sets V
            (0xD0, 0x90, false, 0x60, ".V.C"), // -48 + -112: both negative, result positive
            (0x50, 0xD0, false, 0x20, "...C"), // mixed signs never overflow
        ];
        for (a, m, c, result, flags) in cases {
            let (cpu, _) = run(&[0x69, m], 1, |cpu, _| {
                cpu.reg_a = a;
                cpu.set_carry(c);
            });
            let ctx = format!("ADC ${:02X} + ${:02X} + {}", a, m, c as u8);
            assert_eq!(cpu.reg_a, result, "{}", ctx);
            assert_eq!(nvzc(&cpu), flags, "{}", ctx);
        }
    }

    #[test]
    fn sbc_borrow_and_overflow() {
        // (A, M, carry in, expected A, flags NVZC). C set going in means "no borrow"
        let cases = [
            (0x05, 0x03, true, 0x02, "...C"),  // no borrow out
            (0x05, 0x03, false, 0x01, "...C"), // borrow in subtracts 1 more
            (0x03, 0x05, true, 0xFE, "N..."),  // result negative, borrow out (C clear)
            (0x05, 0x05, true, 0x00, "..ZC"),
            (0x50, 0xB0, true, 0xA0, "NV.."), // 80 - (-80) = 160: signed overflow
            (0xD0, 0x70, true, 0x60, ".V.C"), // -48 - 112 = -160: signed overflow
        ];
        for (a, m, c, result, flags) in cases {
            let (cpu, _) = run(&[0xe9, m], 1, |cpu, _| {
                cpu.reg_a = a;
                cpu.set_carry(c);
            });
            let ctx = format!("SBC ${:02X} - ${:02X} with C={}", a, m, c as u8);
            assert_eq!(cpu.reg_a, result, "{}", ctx);
            assert_eq!(nvzc(&cpu), flags, "{}", ctx);
        }
    }

    #[test]
    fn adc_absolute_y_page_crossed() {
        assert_eq!(cycles_for(&[0x79, 0xff, 0x10], |c| c.reg_y = 1), 5);
    }

    #[test]
    fn logic_ops() {
        // AND, ORA, EOR #$0F with A = $F3
        for (code, result) in [(0x29, 0x03), (0x09, 0xFF), (0x49, 0xFC)] {
            let (cpu, _) = run(&[code, 0x0f], 1, |c, _| c.reg_a = 0xF3);
            assert_eq!(cpu.reg_a, result, "opcode ${:02X}", code);
            assert_eq!(cpu.sr & FLAG_NEGATIVE != 0, result & 0x80 != 0);
        }
        let (cpu, _) = run(&[0x29, 0x00], 1, |c, _| c.reg_a = 0xFF); // AND #$00
        assert_eq!(nvzc(&cpu), "..Z.");
    }

    #[test]
    fn bit_sets_flags_from_memory() {
        // BIT $10 with M = $C0: N and V come from M, Z from A & M
        let (cpu, _) = run(&[0x24, 0x10], 1, |c, b| {
            c.reg_a = 0x01;
            b.write(0x0010, 0xC0);
        });
        assert_eq!(nvzc(&cpu), "NVZ.");
        assert_eq!(cpu.reg_a, 0x01, "BIT leaves A alone");

        let (cpu, _) = run(&[0x24, 0x10], 1, |c, b| {
            c.reg_a = 0x01;
            b.write(0x0010, 0x01);
        });
        assert_eq!(nvzc(&cpu), "....");
    }

    #[test]
    fn shifts_and_rotates_accumulator() {
        // (opcode, A, carry in, expected A, flags NVZC)
        let cases = [
            (0x0a, 0x81, false, 0x02, "...C"), // ASL A: bit 7 into C
            (0x4a, 0x81, false, 0x40, "...C"), // LSR A: bit 0 into C
            (0x2a, 0x80, true, 0x01, "...C"),  // ROL A: C into bit 0, bit 7 into C
            (0x6a, 0x01, true, 0x80, "N..C"),  // ROR A: C into bit 7, bit 0 into C
            (0x0a, 0x40, false, 0x80, "N..."),
            (0x4a, 0x01, false, 0x00, "..ZC"),
        ];
        for (code, a, c, result, flags) in cases {
            let (cpu, _) = run(&[code], 1, |cpu, _| {
                cpu.reg_a = a;
                cpu.set_carry(c);
            });
            let ctx = format!("opcode ${:02X} on ${:02X} with C={}", code, a, c as u8);
            assert_eq!(cpu.reg_a, result, "{}", ctx);
            assert_eq!(nvzc(&cpu), flags, "{}", ctx);
            assert_eq!(cpu.total_cycles, 2, "{}", ctx);
        }
    }

    #[test]
    fn read_modify_write_memory() {
        // (program, starting value at $10, expected value, cycles)
        let cases: [(&[u8], u8, u8, u64); 6] = [
            (&[0x06, 0x10], 0x41, 0x82, 5),       // ASL $10
            (&[0x46, 0x10], 0x82, 0x41, 5),       // LSR $10
            (&[0xe6, 0x10], 0xFF, 0x00, 5),       // INC $10 wraps
            (&[0xc6, 0x10], 0x00, 0xFF, 5),       // DEC $10 wraps
            (&[0xf6, 0x0f], 0x07, 0x08, 6),       // INC $0F,X with X = 1
            (&[0xfe, 0x0f, 0x00], 0x07, 0x08, 7), // INC $000F,X: always 7, no page penalty
        ];
        for (program, before, after, cycles) in cases {
            let (cpu, bus) = run(program, 1, |c, b| {
                c.reg_x = 1;
                b.write(0x0010, before);
            });
            assert_eq!(bus.read(0x0010), after, "program {:02X?}", program);
            assert_eq!(cpu.total_cycles, cycles, "program {:02X?}", program);
            assert_eq!(cpu.reg_a, 0, "memory forms leave A alone");
        }
    }

    #[test]
    fn y_register_and_compares() {
        let (cpu, bus) = run(&[0xa0, 0x80, 0x84, 0x10, 0xc8, 0x88, 0x88], 5, |_, _| {}); // LDY #$80; STY $10; INY; DEY; DEY
        assert_eq!(bus.read(0x0010), 0x80);
        assert_eq!(cpu.reg_y, 0x7F);
        assert_eq!(nvzc(&cpu), "....");

        // CPX / CPY #$10 against 5, 16, 32
        for code in [0xe0, 0xc0] {
            for (reg, flags) in [(0x05, "N..."), (0x10, "..ZC"), (0x20, "...C")] {
                let (cpu, _) = run(&[code, 0x10], 1, |c, _| {
                    c.reg_x = reg;
                    c.reg_y = reg;
                });
                assert_eq!(nvzc(&cpu), flags, "opcode ${:02X} with ${:02X}", code, reg);
            }
        }
    }

    #[test]
    fn flag_instructions() {
        let (cpu, _) = run(&[0x38, 0xf8, 0x58], 3, |_, _| {}); // SEC; SED; CLI
        assert_ne!(cpu.sr & FLAG_CARRY, 0);
        assert_ne!(cpu.sr & FLAG_DECIMAL, 0);
        assert_eq!(cpu.sr & FLAG_INTERRUPT_DISABLE, 0);
        let (cpu, _) = run(&[0x18, 0xb8], 2, |c, _| c.sr = 0xFF); // CLC; CLV
        assert_eq!(cpu.sr & (FLAG_CARRY | FLAG_OVERFLOW), 0);
    }

    #[test]
    fn every_branch_follows_its_flag() {
        // (opcode, flag, branches when the flag is set?)
        let branches = [
            (0x90, FLAG_CARRY, false),    // BCC
            (0xb0, FLAG_CARRY, true),     // BCS
            (0xd0, FLAG_ZERO, false),     // BNE
            (0xf0, FLAG_ZERO, true),      // BEQ
            (0x10, FLAG_NEGATIVE, false), // BPL
            (0x30, FLAG_NEGATIVE, true),  // BMI
            (0x50, FLAG_OVERFLOW, false), // BVC
            (0x70, FLAG_OVERFLOW, true),  // BVS
        ];
        for (code, flag, when_set) in branches {
            for set in [false, true] {
                let (cpu, _) = run(&[code, 0x10], 1, |c, _| c.sr = if set { flag } else { 0 });
                let taken = set == when_set;
                let expected = if taken { 0x0612 } else { 0x0602 };
                assert_eq!(
                    cpu.pc, expected,
                    "opcode ${:02X} with flag set={}",
                    code, set
                );
                assert_eq!(cpu.total_cycles, 2 + taken as u64);
            }
        }
    }

    #[test]
    fn tsx_copies_sp_and_sets_flags() {
        let (cpu, _) = run(&[0xba], 1, |c, _| c.sp = 0x80);
        assert_eq!(cpu.reg_x, 0x80);
        assert_eq!(nvzc(&cpu), "N...");
    }

    #[test]
    fn brk_then_rti_round_trips() {
        // BRK at $0600 jumps through $FFFE to $0700, where RTI returns to $0602
        let (cpu, bus) = run(&[0x00], 1, |c, b| {
            b.load(0xFFFE, &[0x00, 0x07]);
            b.write(0x0700, 0x40); // RTI
            c.sr = FLAG_CARRY | FLAG_UNUSED;
        });
        assert_eq!(cpu.pc, 0x0700);
        assert_eq!(cpu.sp, 0xFA);
        assert_eq!(bus.read(0x01FD), 0x06, "return address high byte");
        assert_eq!(bus.read(0x01FC), 0x02, "return address low byte (BRK + 2)");
        assert_eq!(
            bus.read(0x01FB),
            FLAG_CARRY | FLAG_UNUSED | FLAG_BREAK,
            "pushed status"
        );
        assert_ne!(cpu.sr & FLAG_INTERRUPT_DISABLE, 0, "BRK sets I");
        assert_eq!(cpu.total_cycles, 7);

        let mut cpu = cpu;
        let mut bus = bus;
        cpu.step(1, &mut bus);
        assert_eq!(cpu.pc, 0x0602);
        assert_eq!(cpu.sp, 0xFD);
        assert_eq!(
            cpu.sr,
            FLAG_CARRY | FLAG_UNUSED,
            "RTI restores the status (B dropped)"
        );
        assert_eq!(cpu.total_cycles, 7 + 6);
    }

    #[test]
    fn nmi_pushes_state_and_rti_returns() {
        // NMI vector -> $0700, where an RTI waits
        let (mut cpu, mut bus) = run(&[0xea], 0, |c, b| {
            b.load(0xFFFA, &[0x00, 0x07]);
            b.write(0x0700, 0x40); // RTI
            c.sr = FLAG_CARRY | FLAG_UNUSED; // I clear
        });
        cpu.nmi(&mut bus);
        assert_eq!(cpu.pc, 0x0700);
        assert_eq!(cpu.sp, 0xFA);
        assert_eq!(bus.read(0x01FD), 0x06, "PC high byte");
        assert_eq!(bus.read(0x01FC), 0x00, "PC low byte");
        // Pushed before I is set, and with B clear
        assert_eq!(bus.read(0x01FB), FLAG_CARRY | FLAG_UNUSED);
        assert_ne!(cpu.sr & FLAG_INTERRUPT_DISABLE, 0, "NMI sets I");

        cpu.step(1, &mut bus); // the interrupt's 7 cycles, then RTI's 6
        assert_eq!(cpu.total_cycles, 7 + 6);
        assert_eq!(cpu.pc, 0x0600);
        assert_eq!(cpu.sp, 0xFD);
        assert_eq!(cpu.sr, FLAG_CARRY | FLAG_UNUSED, "RTI turns I back off");
    }

    #[test]
    fn nmi_ignores_the_i_flag() {
        let (mut cpu, mut bus) = run(&[], 0, |c, b| {
            b.load(0xFFFA, &[0x00, 0x07]);
            c.set_interrupt_disable(true);
        });
        cpu.nmi(&mut bus);
        assert_eq!(cpu.pc, 0x0700);
    }

    #[test]
    fn irq_respects_the_i_flag() {
        let (mut cpu, mut bus) = run(&[], 0, |c, b| {
            b.load(0xFFFE, &[0x00, 0x08]);
            c.set_interrupt_disable(true);
        });
        assert!(!cpu.irq(&mut bus), "masked by I");
        assert_eq!(cpu.pc, 0x0600);
        assert_eq!(cpu.sp, 0xFD, "nothing pushed");

        cpu.set_interrupt_disable(false);
        assert!(cpu.irq(&mut bus));
        assert_eq!(cpu.pc, 0x0800);
        assert_eq!(
            bus.read(0x01FB) & FLAG_BREAK,
            0,
            "B clear for hardware interrupts"
        );
    }

    #[test]
    fn log_writes_one_line_per_instruction() {
        let path = std::env::temp_dir().join(format!("cpu_log_test_{}.log", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        {
            let (mut cpu, mut bus) = run(&[0xa9, 0x80, 0xea], 0, |_, b| {
                b.load(0xFFFA, &[0x00, 0x07]);
            });
            cpu.enable_log(&path).unwrap();
            cpu.step(2, &mut bus); // LDA #$80; NOP
            cpu.nmi(&mut bus);
        } // dropping the CPU flushes the log
        let log = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 3, "{}", log);
        assert_eq!(
            lines[0],
            "         0 PC:0600 A9 80     LDA #$80      A:00 X:00 Y:00 ..UB.... STKP:FD"
        );
        // Second line shows the state after LDA: A = $80 and N set
        assert_eq!(
            lines[1],
            "         2 PC:0602 EA        NOP           A:80 X:00 Y:00 N.UB.... STKP:FD"
        );
        assert_eq!(lines[2], "         4 ---- NMI from 0603 to 0700");
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
