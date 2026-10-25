# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

An early-stage NES emulator in Rust (edition 2024). The 6502 CPU implements all 151 official opcodes (no unofficial/illegal ones, no decimal mode since the NES's 2A03 lacks it). `irq()`/`nmi()` exist but nothing triggers them yet except the debugger's I/N keys. There is no PPU, APU, or mapper layer yet.

## Commands

- Build: `cargo build`
- Open the CPU debugger on the built-in dummy program (loaded at `$0600`): `cargo run`
- Open the CPU debugger on a ROM: `cargo run -- DonkeyKong.nes`. Other test ROMs are in the repo root as `.zip` files and must be extracted first. ROMs (`*.nes`, `*.zip`) are gitignored and kept local only.
- Log every instruction (olcNES LOGMODE): add `--log cpu.log` to any run, e.g. `cargo run -- --log cpu.log DonkeyKong.nes`. One line per instruction with the state *before* it runs (nestest convention), plus a line per interrupt.
- Disassemble: `cargo run -- --disasm <rom.nes> [count] [addr]` lists `count` instructions (default 16) from hex `addr` (default: reset vector) and marks which ones are implemented.
- Tests: `cargo test` (cycle-timing tests live in `cpu.rs`; run a single test with `cargo test <name>`)
- Lint/format: `cargo clippy`, `cargo fmt`

Running (without `--disasm`) opens the debugger window: SPACE steps one instruction, R resets, I fires an IRQ (ignored while the I flag is set), N fires an NMI, ESC quits. The CPU's debug trace still prints to stdout as you step.

## Architecture

- **`bus.rs`**: `Bus` holds a flat 64KB `ram` array and is the only way components touch memory: `read(addr)`, `write(addr, data)`, plus `load` (copy a slice in) and `print_range` (hex dump). There is no RAM mirroring, memory-mapped I/O, or mapper indirection yet; those belong in `read`/`write`. `main` fakes the PPU by writing `$80` to `$2002` so vblank-wait loops exit.
- **`opcode.rs`**: a `lazy_static` table (`CPU_OPS_CODES`) of `OpCode { code, mnemonic, len, cycles, mode }` (`cycles` = base cost), indexed by `OPCODES_MAP: HashMap<u8, &OpCode>`. Every new instruction needs an entry here. Unknown opcodes panic in `execute`.
- **`cpu.rs`**: `execute(bus)` fetches one opcode, looks it up in `OPCODES_MAP`, and dispatches by matching on the **mnemonic string**. New handlers must also be added to `IMPLEMENTED`, which the disassembler uses. A test in `disasm.rs` checks `opcode.rs` against the disassembler's independent opcode list (mnemonic, length, mode) and `IMPLEMENTED`.
  - Shared helpers: `read_operand` (value + page-cross cycle), `compare` (CMP/CPX/CPY), `add_with_carry` (ADC, and SBC as ADC of the inverted operand), and `read_modify_write` (ASL/LSR/ROL/ROR/INC/DEC on A in `Accumulator` mode or on memory).
  - Timing follows olcNES: `clock(bus)` runs a whole instruction on its first cycle, then counts down `cycles`; `complete()` is true when it reaches 0. `step(n, bus)` clocks through `n` whole instructions. `total_cycles` counts since power-on, and `reset()` adds the 7-cycle reset sequence.
  - An instruction's cost is `OpCode.cycles` plus whatever its handler returns: indexed reads (`lda`/`ldx`) return 1 when `operand_address` reports a page crossing, and `branch` returns +1 if taken, +1 more if the target is on another page. Stores never pay the page penalty (their table cost is already worst-case).
  - `operand_address(mode, bus)` resolves all addressing modes to a `(u16, page_crossed)` effective address (Immediate is PC+1; zero page indexing and `(zp),Y`/`(zp,X)` pointers wrap within page zero).
  - PC advance rule: control-flow instructions (Relative-mode branches, `JMP`, `JSR`, `RTS`) always set PC themselves, including a not-taken branch moving to the next instruction. Everything else advances by `opcode.len` after the handler runs. Add new control-flow mnemonics (e.g. `RTI`, `BRK`) to `sets_pc` in `execute`.
  - Stack is page one (`$0100–$01FF`) and grows downward. JSR pushes PC+2 (high byte first) and RTS pops and adds 1, which matches real 6502 behavior.
  - `irq()`/`nmi()` share `interrupt()`: push PC, push status with B clear, *then* set I (olcNES sets I first, which is wrong), jump through `$FFFE`/`$FFFA`, add 7 cycles. `brk()` is the same but pushes PC+2 with B set. `rti()` undoes them.
  - `reset()` loads PC from the reset vector at `$FFFC/$FFFD`. `CPU::new()` defaults PC to `$0600` for the dummy program.
  - Handlers print a lot of debug trace (operands, memory before/after, stack dumps, register state via the `{:#X}` `UpperHex` impl).
- **`disasm.rs`**: linear-sweep disassembler. `decode`/`decode_range` return `Instruction` structs (used by the debugger); `disassemble` formats them as text lines for `--disasm`. Modes use olcNES abbreviations (`IMM`, `ABS`, `REL`, ...). Doesn't follow jumps.
- **`debugger.rs`**: the olcNES Part#2 debug screen in a `pixels` + `winit` window (680x480 at 2x): RAM page `$0000`, the RAM page containing PC, registers/flags/cycles, and the next 25 instructions from PC (current in cyan, unimplemented in red). Stepping is refused with a message if the next instruction isn't implemented, instead of panicking.
- **`canvas.rs`** / **`font.rs`**: frame-buffer drawing (`clear`, `draw`, `draw_string`) and the olcPixelGameEngine 8x8 font, ported from `pixels_test`.
- **`rom.rs`**: parses iNES files (header, optional trainer, PRG, CHR, mapper number). `load_into` copies PRG ROM to `$8000` and mirrors a 16KB PRG into `$C000` so the vectors line up. CHR ROM is parsed but not used yet. Only mapper-0 (NROM) layouts load correctly.

## Other directories

- **`pixels_test/`**: a separate crate (own `Cargo.toml`, run with `cargo run` inside it) for trying out the display with `pixels` + `winit`. It draws text with an 8x8 bitmap font ported from olcPixelGameEngine (`src/font.rs`) on the dark-blue background used by the olcNES Part#2 demo.
- **`olcNES/`**: git submodule of OneLoneCoder's C++ olcNES, used as a reference implementation. Run `git submodule update --init` after cloning. Local build files in it are ignored (`ignore = untracked`).
