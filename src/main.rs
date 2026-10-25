mod cpu;
mod memory;
mod opcode;

use crate::{cpu::CPU, memory::Mem};
#[macro_use]
extern crate lazy_static;

fn main() {
    let mut system_mem = Mem { mem: [0; 65536] };
    let mut cpu = CPU::new();

    // Load some dummy program code
    let program_code = vec![0xA2, 0x01, 0xA9, 0xAA, 0x95, 0xA5, 0xE8, 0x95, 0xA0];
    system_mem.load(0x0600, &program_code);

    // Option B: Print a sequence/range of memory
    system_mem.print_range(0x0600, 16);

    cpu.step(3, &mut system_mem);
}
