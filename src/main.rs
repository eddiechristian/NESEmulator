mod bus;
mod canvas;
mod cpu;
mod debugger;
mod disasm;
mod font;
mod opcode;
mod rom;

use crate::{bus::Bus, cpu::CPU, rom::Rom};
use std::env;
#[macro_use]
extern crate lazy_static;

fn main() {
    let mut bus = Bus::new();
    let mut cpu = CPU::new();

    // Usage: cargo run -- --log cpu.log [other args]
    // LOGMODE: writes every instruction the CPU runs to the given file (see CPU::enable_log)
    let mut args: Vec<String> = env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--log") {
        if i + 1 >= args.len() {
            eprintln!("Usage: --log <file>");
            std::process::exit(1);
        }
        let path = args.remove(i + 1);
        args.remove(i);
        cpu.enable_log(&path).unwrap_or_else(|e| {
            eprintln!("Error: could not create log {}: {}", path, e);
            std::process::exit(1);
        });
        println!("Logging CPU to {}", path);
    }
    let arg = |n: usize| args.get(n).cloned();

    // Usage: cargo run -- --disasm path/to/game.nes [count] [addr]
    // Lists `count` instructions from `addr` (hex, e.g. EFFE, $EFFE or 0xEFFE) and whether
    // each one is implemented. Without `addr`, starts at the reset vector
    if arg(1).as_deref() == Some("--disasm") {
        let path = arg(2).unwrap_or_else(|| {
            eprintln!("Usage: --disasm <rom.nes> [count] [addr]");
            std::process::exit(1);
        });
        let count = match arg(3) {
            Some(n) => n.parse().unwrap_or_else(|_| {
                eprintln!("Error: count must be a number, got {}", n);
                std::process::exit(1);
            }),
            None => 16,
        };
        let rom = Rom::from_file(&path).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        rom.load_into(&mut bus);
        let start = match arg(4) {
            Some(a) => {
                let hex = a.trim_start_matches('$').trim_start_matches("0x");
                u16::from_str_radix(hex, 16).unwrap_or_else(|_| {
                    eprintln!("Error: addr must be a hex address like EFFE, got {}", a);
                    std::process::exit(1);
                })
            }
            None => {
                cpu.reset(&bus);
                cpu.pc as u16
            }
        };
        for line in disasm::disassemble(&bus, start, count) {
            println!("{}", line);
        }
        return;
    }

    // Usage: cargo run -- path/to/game.nes
    // Opens the CPU debugger window. With no argument, loads the dummy program instead
    match arg(1) {
        Some(path) => {
            let rom = Rom::from_file(&path).unwrap_or_else(|e| {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            });
            rom.load_into(&mut bus);

            //temporary hack until we get PPU.
            bus.write(0x2002, 0x80);
        }
        None => {
            // Load some dummy program code
            let program_code = vec![
                0x20, 0x09, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA2, 0x01, 0xA9, 0xAA, 0x95,
                0xA5, 0xE8, 0x95, 0xA0,
            ];
            bus.load(0x0600, &program_code);

            // Point the reset vector at it so R in the debugger restarts it
            bus.load(0xFFFC, &[0x00, 0x06]);
        }
    }

    cpu.reset(&bus);
    debugger::run(cpu, bus);
}
