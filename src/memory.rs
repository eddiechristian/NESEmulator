pub struct Mem {
    pub mem: [u8; 65536],
}

impl Mem {
    pub fn load(&mut self, addr: u16, data: &[u8]) {
        let start = addr as usize;

        // Calculate where the data should end
        let end = start + data.len();

        // Prevent panics by ensuring we don't write past the 65,536 byte limit
        if end <= self.mem.len() {
            // Target the specific slice of your memory array and copy the data over
            self.mem[start..end].copy_from_slice(data);
        } else {
            // Optional: Handle the out-of-bounds error safely
            eprintln!("Error: Data slice is too large to fit into memory at this address.");
        }
    }

    pub fn print_range(&self, start_addr: u16, length: u16) {
        let start = start_addr as usize;
        let end = start + length as usize;

        // Ensure we don't read past the end of the array
        let safe_end = end.min(self.mem.len());

        println!("--- Memory Dump starting at {:#06X} ---", start_addr);
        for (i, byte) in self.mem[start..safe_end].iter().enumerate() {
            let current_addr = start + i;
            if i % 16 == 0 {
                print!("{:#06x}  ", start);
            }
            print!("{:#04x} ", byte);

            // New line every 8 bytes for readability
        }
        println!();
    }
}
