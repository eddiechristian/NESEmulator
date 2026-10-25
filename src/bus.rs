pub struct Bus {
    pub ram: [u8; 1024 * 64],
}

impl Bus {
    pub fn new() -> Self {
        Bus {
            ram: [0; 1024 * 64],
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        self.ram[addr as usize] = data;
    }

    // Writes `data` starting at `addr`, stopping at the top of the address space
    pub fn load(&mut self, addr: u16, data: &[u8]) {
        if addr as usize + data.len() > self.ram.len() {
            eprintln!("Error: Data slice is too large to fit into memory at this address.");
            return;
        }
        for (i, &byte) in data.iter().enumerate() {
            self.write(addr + i as u16, byte);
        }
    }

    pub fn print_range(&self, start_addr: u16, length: u16) {
        println!("--- Memory Dump starting at {:#06X} ---", start_addr);
        for i in 0..length {
            let Some(addr) = start_addr.checked_add(i) else {
                break;
            };
            if i % 16 == 0 {
                print!("{:#06x}  ", addr);
            }
            print!("{:#04x} ", self.read(addr));
        }
        println!();
    }
}
