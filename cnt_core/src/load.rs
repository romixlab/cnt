use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use object::{File, Object, ObjectSection, ObjectSymbol, SectionIndex};
use anyhow::anyhow;
use serde::Deserialize;
use crate::{location, Buffer, Counter, Counters, Location, Severity, Storage, Ty};

impl Counters {
    pub fn load_elf(path: &Path) -> anyhow::Result<Self> {
        let elf_bytes = std::fs::read(path)?;
        let elf = File::parse(elf_bytes.as_slice())?;
        let ram_section_idx = elf.section_by_name(".counters_ram").map(|s| s.index());
        let bkp_section_idx = elf.section_by_name(".counters_bkp").map(|s| s.index());
        if ram_section_idx.is_none() && bkp_section_idx.is_none() {
            return Err(anyhow!("No .counters_ram or .counters_bkp section found, cnt crate or cnt.x linker script is not used"));
        };

        let mut cnt_ram_buffer = None;
        let mut cnt_bkp_buffer = None;
        for symbol in elf.symbols() {
            let Ok(symbol_name) = symbol.name() else {
                continue;
            };
            if symbol_name == "_CNT_RAM_BUFFER" {
                cnt_ram_buffer = Some(Buffer { addr: symbol.address(), size: symbol.size() });
                continue;
            }
            if symbol_name == "_CNT_BKP_BUFFER" {
                cnt_bkp_buffer = Some(Buffer { addr: symbol.address(), size: symbol.size() });
                continue;
            }
        }
        
        let mut raw_symbols_ram = vec![];
        let mut dedup_str = vec![];
        let mut ram_counters = if let Some(ram_section_idx) = ram_section_idx {
            collect_counters(&elf, ram_section_idx, &mut raw_symbols_ram, &mut dedup_str)
        } else {
            BTreeMap::new()
        };
        if !ram_counters.is_empty() {
            let locations = location::get_locations(&elf, &raw_symbols_ram)?;
            fill_locations(&mut ram_counters, locations);
            fill_addresses(&mut ram_counters, cnt_ram_buffer, "_CNT_RAM_BUFFER")?;
        }
        
        let mut raw_symbols_bkp = vec![];
        let mut bkp_counters = if let Some(bkp_section_idx) = bkp_section_idx {
            collect_counters(&elf, bkp_section_idx, &mut raw_symbols_bkp, &mut dedup_str)
        } else {
            BTreeMap::new()
        };
        if !bkp_counters.is_empty() {
            let locations = location::get_locations(&elf, &raw_symbols_bkp)?;
            fill_locations(&mut bkp_counters, locations);
            fill_addresses(&mut bkp_counters, cnt_bkp_buffer, "_CNT_BKP_BUFFER")?;
        }
 

        Ok(Self {
            ram_counters,
            bkp_counters,
            cnt_ram_buffer,
            cnt_bkp_buffer,
        })
    }

    pub fn ram_counters(&self) -> &BTreeMap<u64, Counter> {
        &self.ram_counters
    }

    pub fn bkp_counters(&self) -> &BTreeMap<u64, Counter> {
        &self.bkp_counters
    }

    pub fn ram_buffer(&self) -> Option<Buffer> {
        self.cnt_ram_buffer
    }

    pub fn bkp_buffer(&self) -> Option<Buffer> {
        self.cnt_bkp_buffer
    }
}

fn collect_counters<'f>(elf: &'f File, filter_section_idx: SectionIndex, raw_symbols: &mut Vec<&'f str>, dedup_str: &mut Vec<Arc<String>>) -> BTreeMap<u64, Counter> {
    let mut counters = BTreeMap::new();
    for symbol in elf.symbols() {
        let (Ok(symbol_name), Some(section_idx)) = (symbol.name(), symbol.section_index()) else {
            continue;
        };
        if section_idx != filter_section_idx {
            continue;
        }
        if !symbol_name.starts_with("{") {
            continue;
        }
        raw_symbols.push(symbol_name);
        let symbol_addr = symbol.address();
        let symbol: anyhow::Result<Symbol, _> = serde_json::from_str(&symbol_name);
        let Ok(symbol) = symbol else {
            continue;
        };
        let ty = if symbol.ty == "u32" {
            Ty::U32
        } else if symbol.ty == "u64_lo" {
            Ty::U64
        } else {
            continue;
        };
        let group = dedup(dedup_str, symbol.group);
        let package = dedup(dedup_str, symbol.package);
        let crate_name = dedup(dedup_str, symbol.crate_name);
        counters.insert(symbol_addr, Counter {
            group,
            crate_name,
            package,
            name: symbol.name,
            storage: symbol.storage,
            ty,
            severity: symbol.severity,
            location: None,
            buf: Buffer { addr: 0, size: 0 },
        });
    }
    counters
}

fn fill_locations(counters: &mut BTreeMap<u64, Counter>, locations: BTreeMap<u64, Location>) {
    for (addr, loc) in locations.into_iter() {
        if let Some(entry) = counters.get_mut(&addr) {
            entry.location = Some(loc);
        }
    }
}

fn fill_addresses(counters: &mut BTreeMap<u64, Counter>, buffer: Option<Buffer>, name: &'static str) -> anyhow::Result<()> {
    let Some(buffer) = buffer else {
        return Err(anyhow!("No {name} symbol found, cnt crate or cnt.x linker script is not used"));
    };
    for (idx, counter) in counters.iter_mut() {
        counter.buf = Buffer { addr: buffer.addr + idx * counter.ty.len() as u64, size: counter.ty.len() as u64 };
    }
    Ok(())
}

pub(crate) fn dedup<T: PartialEq>(seen: &mut Vec<Arc<T>>, current: T) -> Arc<T> {
    if let Some(v) = seen.iter().find(|v| v.as_ref() == &current) {
        v.clone()
    } else {
        let v = Arc::new(current);
        seen.push(v.clone());
        v
    }
}

#[derive(Deserialize)]
pub struct Symbol {
    package: String,
    group: String,
    storage: Storage,
    name: String,
    ty: String,
    severity: Severity,
    // disambiguator: u64,
    crate_name: String,
}
