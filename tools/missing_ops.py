#!/usr/bin/env python3
"""Lists the instructions a ROM uses that the emulator doesn't implement yet.

Follows code from the reset/NMI/IRQ vectors (branches, JMP, JSR), so data tables
aren't mistaken for code. It can't follow JMP ($xxxx) or jump tables, so a few
routines may be missed. Opcode info is read from src/disasm.rs, and what's
implemented from src/opcode.rs + IMPLEMENTED in src/cpu.rs, so it stays in sync.

Usage: python3 tools/missing_ops.py ROM.nes
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "src"
LEN = {"Implied": 1, "Accumulator": 1, "Absolute": 3, "AbsoluteX": 3, "AbsoluteY": 3, "Indirect": 3}
ABBR = {"Implied": "IMP", "Accumulator": "IMP", "Immediate": "IMM", "ZeroPage": "ZP0",
        "ZeroPageX": "ZPX", "ZeroPageY": "ZPY", "Absolute": "ABS", "AbsoluteX": "ABX",
        "AbsoluteY": "ABY", "Indirect": "IND", "IndirectX": "IZX", "IndirectY": "IZY",
        "Relative": "REL"}


def load_tables():
    disasm = (SRC / "disasm.rs").read_text()
    ops = {int(c, 16): (name, mode)
           for c, name, mode in re.findall(r'\(0x([0-9A-Fa-f]{2}), "([A-Z]{3})", Mode::(\w+)\)', disasm)}
    table = {int(c, 16) for c in re.findall(r'OpCode::new\(0x([0-9a-fA-F]{2})', (SRC / "opcode.rs").read_text())}
    cpu = (SRC / "cpu.rs").read_text()
    implemented_names = set(re.findall(r'"([A-Z]{3})"', re.search(r"IMPLEMENTED: &\[&str\] = &\[(.*?)\];", cpu, re.S).group(1)))
    implemented = {c for c in table if c in ops and ops[c][0] in implemented_names}
    return ops, implemented


def load_prg(path):
    data = Path(path).read_bytes()
    assert data[:4] == b"NES\x1a", "not an iNES file"
    start = 16 + (512 if data[6] & 4 else 0)
    prg = data[start:start + data[4] * 0x4000]
    # 16KB PRG is mirrored into $C000; 32KB fills $8000-$FFFF
    return prg * 2 if len(prg) == 0x4000 else prg


def walk(prg, ops):
    read = lambda a: prg[(a - 0x8000) % len(prg)]
    word = lambda a: read(a) | read(a + 1) << 8
    vectors = {"NMI": word(0xFFFA), "RESET": word(0xFFFC), "IRQ": word(0xFFFE)}

    seen, todo, unknown, indirect = {}, list(vectors.values()), set(), set()
    while todo:
        addr = todo.pop()
        while 0x8000 <= addr <= 0xFFFF and addr not in seen:
            code = read(addr)
            if code not in ops:
                unknown.add(addr)  # probably data reached by a jump table
                break
            name, mode = ops[code]
            seen[addr] = code
            size = LEN.get(mode, 2)
            nxt = addr + size
            if mode == "Relative":
                off = read(addr + 1)
                todo.append(nxt + (off - 256 if off > 127 else off))
            if name == "JMP":
                if mode == "Absolute":
                    todo.append(word(addr + 1))
                else:
                    indirect.add(addr)
                break
            if name == "JSR":
                todo.append(word(addr + 1))
            if name in ("RTS", "RTI", "BRK"):
                break
            addr = nxt
    return vectors, seen, unknown, indirect


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    ops, implemented = load_tables()
    vectors, seen, unknown, indirect = walk(load_prg(sys.argv[1]), ops)

    missing = defaultdict(lambda: defaultdict(list))  # name -> mode -> [addresses]
    for addr, code in seen.items():
        if code not in implemented:
            name, mode = ops[code]
            missing[name][ABBR[mode]].append(addr)

    print(f"Vectors: " + ", ".join(f"{k} ${v:04X}" for k, v in vectors.items()))
    print(f"Reachable instructions found: {len(seen)}, "
          f"implemented: {sum(c in implemented for c in seen.values())}, "
          f"missing: {sum(c not in implemented for c in seen.values())}")
    print(f"Distinct opcodes used: {len(set(seen.values()))}, "
          f"still missing: {sum(len(m) for m in missing.values())} "
          f"across {len(missing)} mnemonics\n")

    rows = sorted(missing.items(), key=lambda kv: -sum(len(a) for a in kv[1].values()))
    print(f"{'Op':<4} {'Uses':>4}  Modes (uses, first address)")
    for name, modes in rows:
        detail = ", ".join(f"{m} x{len(a)} @${min(a):04X}" for m, a in sorted(modes.items(), key=lambda kv: -len(kv[1])))
        print(f"{name:<4} {sum(len(a) for a in modes.values()):>4}  {detail}")

    if indirect:
        print("\nJMP ($xxxx) at " + ", ".join(f"${a:04X}" for a in sorted(indirect))
              + " -- targets not followed, so code behind them may be missing from this list")
    if unknown:
        print(f"{len(unknown)} paths ran into non-opcode bytes (likely data after a jump-table JSR)")


if __name__ == "__main__":
    main()
