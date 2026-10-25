# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

An early-stage NES emulator in Rust (edition 2024). Only a handful of 6502 CPU instructions exist so far (`IMPLEMENTED` in `cpu.rs`). There is no PPU, APU, or mapper layer yet.

## Commands

- Build: `cargo build`
- Run the built-in dummy program (loaded at `$0600`): `cargo run`
- Run against a ROM: `cargo run -- DonkeyKong.nes`. Other test ROMs are in the repo root as `.zip` files and must be extracted first. ROMs (`*.nes`, `*.zip`) are gitignored and kept local only.
- Disassemble: `cargo run -- --disasm <rom.nes> [count] [addr]` lists `count` instructions (default 16) from hex `addr` (default: reset vector) and marks which ones are implemented.
- Tests: `cargo test` (there are no tests yet; run a single test with `cargo test <name>`)
- Lint/format: `cargo clippy`, `cargo fmt`

`main` currently runs a fixed number of steps (`cpu.step(5, ...)`), so change that count to execute more instructions.

## Architecture

- **`bus.rs`**: `Bus` holds a flat 64KB `ram` array and is the only way components touch memory: `read(addr)`, `write(addr, data)`, plus `load` (copy a slice in) and `print_range` (hex dump). There is no RAM mirroring, memory-mapped I/O, or mapper indirection yet; those belong in `read`/`write`. `main` fakes the PPU by writing `$80` to `$2002` so vblank-wait loops exit.
- **`opcode.rs`**: a `lazy_static` table (`CPU_OPS_CODES`) of `OpCode { code, mnemonic, len, cycles, mode }`, indexed by `OPCODES_MAP: HashMap<u8, &OpCode>`. Every new instruction needs an entry here. Unknown opcodes panic in `step`.
- **`cpu.rs`**: `CPU::step(n, bus)` fetches each opcode, looks it up in `OPCODES_MAP`, and dispatches by matching on the **mnemonic string**. New handlers must also be added to `IMPLEMENTED`, which the disassembler uses.
  - `operand_address(mode, bus)` resolves all addressing modes to a `u16` effective address (Immediate is PC+1; zero page indexing and `(zp),Y`/`(zp,X)` pointers wrap within page zero).
  - PC advance rule: after a handler runs, PC moves forward by `opcode.len` *only if the handler left PC unchanged*. Jumps/branches set PC themselves, but a jump to its own address would be mishandled by this rule.
  - Stack is page one (`$0100–$01FF`) and grows downward. JSR pushes PC+2 (high byte first) and RTS pops and adds 1, which matches real 6502 behavior.
  - `reset()` loads PC from the reset vector at `$FFFC/$FFFD`. `CPU::new()` defaults PC to `$0600` for the dummy program.
  - Handlers print a lot of debug trace (operands, memory before/after, stack dumps, register state via the `{:#X}` `UpperHex` impl).
- **`disasm.rs`**: linear-sweep disassembler behind `--disasm`. Doesn't follow jumps.
- **`rom.rs`**: parses iNES files (header, optional trainer, PRG, CHR, mapper number). `load_into` copies PRG ROM to `$8000` and mirrors a 16KB PRG into `$C000` so the vectors line up. CHR ROM is parsed but not used yet. Only mapper-0 (NROM) layouts load correctly.

## Other directories

- **`pixels_test/`**: a separate crate (own `Cargo.toml`, run with `cargo run` inside it) for trying out the display with `pixels` + `winit`. It draws text with an 8x8 bitmap font ported from olcPixelGameEngine (`src/font.rs`) on the dark-blue background used by the olcNES Part#2 demo.
- **`olcNES/`**: git submodule of OneLoneCoder's C++ olcNES, used as a reference implementation. Run `git submodule update --init` after cloning. Local build files in it are ignored (`ignore = untracked`).
