use std::path::{Path, PathBuf};
use anyhow::{anyhow, Result};
use object::{Object, ObjectSection, ObjectSymbol};

pub struct Counters {

}

impl Counters {
    pub fn load_elf(path: &Path) -> Result<Self> {
        let elf_bytes = std::fs::read(path)?;
        let elf = object::File::parse(elf_bytes.as_slice())?;
        let ram_section_idx = elf.section_by_name(".counters_ram").map(|s| s.index());
        let Some(ram_section_idx) = ram_section_idx else {
            return Err(anyhow!("No .counters_ram section found, counting is disabled or cnt.x linker script is not used"));
        };

        for symbol in elf.symbols() {
            let (Ok(symbol_name), Some(section_idx)) = (symbol.name(), symbol.section_index()) else {
                continue;
            };
            if section_idx != ram_section_idx {
                continue;
            }
            println!("{}", symbol_name); 
        }

        Ok(Self {

        })
    }
}