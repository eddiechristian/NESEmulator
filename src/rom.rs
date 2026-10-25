use crate::bus::Bus;
use std::fs;

const NES_TAG: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A]; // "NES" followed by MS-DOS EOF
const HEADER_SIZE: usize = 16;
const TRAINER_SIZE: usize = 512;
const PRG_BANK_SIZE: usize = 16 * 1024;
const CHR_BANK_SIZE: usize = 8 * 1024;
const PRG_START: u16 = 0x8000;

pub struct Rom {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: u8,
}

impl Rom {
    pub fn from_file(path: &str) -> Result<Rom, String> {
        let data = fs::read(path).map_err(|e| format!("could not read {}: {}", path, e))?;
        Rom::from_bytes(&data)
    }

    // Parses an iNES file: 16 byte header, optional 512 byte trainer, PRG ROM, then CHR ROM
    pub fn from_bytes(data: &[u8]) -> Result<Rom, String> {
        if data.len() < HEADER_SIZE || data[0..4] != NES_TAG {
            return Err("not an iNES file (missing NES header)".to_string());
        }

        let prg_size = data[4] as usize * PRG_BANK_SIZE;
        let chr_size = data[5] as usize * CHR_BANK_SIZE;
        // Mapper number is split: low nibble in the top of byte 6, high nibble in the top of byte 7
        let mapper = (data[7] & 0xF0) | (data[6] >> 4);
        let has_trainer = data[6] & 0b0000_0100 != 0;

        let prg_start = HEADER_SIZE + if has_trainer { TRAINER_SIZE } else { 0 };
        let chr_start = prg_start + prg_size;
        if data.len() < chr_start + chr_size {
            return Err(format!(
                "file is too short: header says {} bytes of PRG and {} of CHR",
                prg_size, chr_size
            ));
        }

        println!(
            "ROM: {}KB PRG, {}KB CHR, mapper {}",
            prg_size / 1024,
            chr_size / 1024,
            mapper
        );

        Ok(Rom {
            prg_rom: data[prg_start..chr_start].to_vec(),
            chr_rom: data[chr_start..chr_start + chr_size].to_vec(),
            mapper,
        })
    }

    // PRG ROM lives at $8000-$FFFF. A 16KB ROM is mirrored into both halves so
    // the vectors at $FFFA-$FFFF still line up
    pub fn load_into(&self, bus: &mut Bus) {
        bus.load(PRG_START, &self.prg_rom);
        if self.prg_rom.len() == PRG_BANK_SIZE {
            bus.load(PRG_START + PRG_BANK_SIZE as u16, &self.prg_rom);
        }
    }
}
