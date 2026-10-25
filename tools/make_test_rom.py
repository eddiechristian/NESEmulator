#!/usr/bin/env python3
"""Builds cpu_test.nes: a mapper-0 iNES ROM that only uses instructions the emulator
implements (CLD DEX JSR LDA LDX RTS SEI STA STX BPL), for stepping through in the
debugger. Each section exercises a different kind of memory access.

Usage: python3 tools/make_test_rom.py [output.nes]   (default: cpu_test.nes)
"""
import sys

ORG = 0x8000

# Each line is (label or None, bytes, comment). Bytes may contain ("abs", label) for a
# 2-byte little-endian address or ("rel", label) for a branch offset.
PROGRAM = [
    ("reset", [0x78], "SEI"),
    (None, [0xD8], "CLD"),

    # 1. Back-to-back writes on three different pages: zero page, $02xx, $03xx
    (None, [0xA9, 0x11], "LDA #$11"),
    (None, [0x85, 0x10], "STA $10"),
    (None, [0x8D, 0x34, 0x02], "STA $0234"),
    (None, [0x8D, 0x77, 0x03], "STA $0377"),

    # 2. Two writes on the same page
    (None, [0xA2, 0x22], "LDX #$22"),
    (None, [0x8E, 0x00, 0x03], "STX $0300"),
    (None, [0x8E, 0x01, 0x03], "STX $0301"),

    # 3. Indexed modes: zero page,X wraps within page zero; absolute,X crosses a page
    (None, [0xA2, 0x05], "LDX #$05"),
    (None, [0x95, 0xFE], "STA $FE,X     -> writes $0003 (wraps)"),
    (None, [0xBD, 0xFE, 0x02], "LDA $02FE,X   -> reads $0303, page crossed: 5 cycles"),
    (None, [0xBD, 0x00, 0x03], "LDA $0300,X   -> reads $0305, same page: 4 cycles"),

    # 4. Indirect: build a pointer to $0400 at $20/$21, write through it, read it back
    (None, [0xA9, 0x00], "LDA #$00"),
    (None, [0x85, 0x20], "STA $20"),
    (None, [0xA9, 0x04], "LDA #$04"),
    (None, [0x85, 0x21], "STA $21"),
    (None, [0xA9, 0x99], "LDA #$99"),
    (None, [0x91, 0x20], "STA ($20),Y   -> writes $0400 (Y = 0)"),
    (None, [0xA2, 0x00], "LDX #$00"),
    (None, [0xA1, 0x20], "LDA ($20,X)   -> reads $0400"),

    # 5. Subroutine: JSR pushes the return address on the stack, RTS pulls it
    (None, [0x20, ("abs", "sub")], "JSR sub"),

    # 6. Countdown loop: writes A to $0504..$0500, exits when DEX takes X to $FF (N set)
    (None, [0xA2, 0x04], "LDX #$04"),
    ("loop", [0x9D, 0x00, 0x05], "STA $0500,X"),
    (None, [0xCA], "DEX"),
    (None, [0x10, ("rel", "loop")], "BPL loop"),

    # Park here forever. LDX #$00 clears N so BPL is always taken
    ("done", [0xA2, 0x00], "LDX #$00"),
    (None, [0x10, ("rel", "done")], "BPL done"),

    ("sub", [0xA9, 0x5A], "LDA #$5A"),
    (None, [0x8D, 0x00, 0x06], "STA $0600"),
    (None, [0x60], "RTS"),
]


def assemble():
    # Pass 1: label addresses
    labels, addr = {}, ORG
    for label, items, _ in PROGRAM:
        if label:
            labels[label] = addr
        addr += sum(2 if isinstance(i, tuple) and i[0] == "abs" else 1 for i in items)

    # Pass 2: emit bytes
    out, addr = bytearray(), ORG
    for _, items, comment in PROGRAM:
        start = len(out)
        for item in items:
            if isinstance(item, tuple):
                kind, target = item
                if kind == "abs":
                    out += labels[target].to_bytes(2, "little")
                else:
                    # Branch offsets are relative to the next instruction (this one is 2 bytes)
                    offset = labels[target] - (addr + 2)
                    assert -128 <= offset <= 127, f"branch to {target} out of range"
                    out.append(offset & 0xFF)
            else:
                out.append(item)
        size = len(out) - start
        print(f"  ${addr:04X}  {' '.join(f'{b:02X}' for b in out[start:]):<9} {comment}")
        addr += size
    return bytes(out), labels


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "cpu_test.nes"
    code, labels = assemble()

    # One 16KB PRG bank (the emulator mirrors it into $C000 too), filled with $EA (NOP)
    prg = bytearray([0xEA] * 0x4000)
    prg[: len(code)] = code
    # Vectors at $FFFA: NMI, RESET, IRQ. NMI and IRQ just park in the done loop
    for i, target in enumerate(["done", "reset", "done"]):
        prg[0x3FFA + i * 2 : 0x3FFC + i * 2] = labels[target].to_bytes(2, "little")

    # iNES header: 1 PRG bank, 1 CHR bank, mapper 0, horizontal mirroring
    header = b"NES\x1a" + bytes([1, 1, 0, 0]) + bytes(8)
    chr_rom = bytes(0x2000)

    with open(path, "wb") as f:
        f.write(header + prg + chr_rom)
    print(f"wrote {path} ({len(header) + len(prg) + len(chr_rom)} bytes), reset -> ${labels['reset']:04X}")


if __name__ == "__main__":
    main()
