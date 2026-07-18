use crate::{Buffer, Counter, Counters, CountersBlock, Location, Severity, Storage, Ty, location};
use anyhow::anyhow;
use object::{File, Object, ObjectSection, ObjectSymbol, SectionIndex};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

impl Counters {
    pub fn load_elf(path: &Path) -> anyhow::Result<Self> {
        let elf_bytes = std::fs::read(path)?;
        let elf = File::parse(elf_bytes.as_slice())?;
        let ram_section_idx = elf.section_by_name(".counters_ram").map(|s| s.index());
        let bkp_section_idx = elf.section_by_name(".counters_bkp").map(|s| s.index());
        if ram_section_idx.is_none() && bkp_section_idx.is_none() {
            return Err(anyhow!(
                "No .counters_ram or .counters_bkp section found, cnt crate or cnt.x linker script is not used"
            ));
        };

        let mut ram_buffer = None;
        let mut bkp_buffer = None;
        for symbol in elf.symbols() {
            let Ok(symbol_name) = symbol.name() else {
                continue;
            };
            if symbol_name == "_CNT_RAM_BUFFER" {
                ram_buffer = Some(Buffer {
                    addr: symbol.address(),
                    size: symbol.size(),
                });
                continue;
            }
            if symbol_name == "_CNT_BKP_BUFFER" {
                bkp_buffer = Some(Buffer {
                    addr: symbol.address(),
                    size: symbol.size(),
                });
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
        let ram_counters = if !ram_counters.is_empty() {
            let locations = location::get_locations(&elf, &raw_symbols_ram)?;
            fill_locations(&mut ram_counters, locations);
            let Some(ram_buffer) = ram_buffer else {
                return Err(anyhow!(
                    "No _CNT_RAM_BUFFER symbol found, cnt crate or cnt.x linker script is not used"
                ));
            };
            fill_addresses(&mut ram_counters, ram_buffer);
            Some(CountersBlock {
                entries: ram_counters,
                buffer: ram_buffer,
                storage: Storage::RAM,
                values: Default::default(),
            })
        } else {
            None
        };

        let mut raw_symbols_bkp = vec![];
        let mut bkp_counters = if let Some(bkp_section_idx) = bkp_section_idx {
            collect_counters(&elf, bkp_section_idx, &mut raw_symbols_bkp, &mut dedup_str)
        } else {
            BTreeMap::new()
        };
        let bkp_counters = if !bkp_counters.is_empty() {
            let locations = location::get_locations(&elf, &raw_symbols_bkp)?;
            fill_locations(&mut bkp_counters, locations);
            let Some(bkp_buffer) = bkp_buffer else {
                return Err(anyhow!(
                    "No _CNT_BKP_BUFFER symbol found, cnt crate or cnt.x linker script is not used"
                ));
            };
            fill_addresses(&mut bkp_counters, bkp_buffer);
            Some(CountersBlock {
                entries: bkp_counters,
                buffer: bkp_buffer,
                storage: Storage::BKP,
                values: Default::default(),
            })
        } else {
            None
        };

        Ok(Self {
            ram_counters,
            bkp_counters,
        })
    }

    pub fn ram_counters(&self) -> Option<&CountersBlock> {
        self.ram_counters.as_ref()
    }

    pub fn bkp_counters(&self) -> Option<&CountersBlock> {
        self.bkp_counters.as_ref()
    }

    pub fn ram_counters_mut(&mut self) -> Option<&mut CountersBlock> {
        self.ram_counters.as_mut()
    }

    pub fn bkp_counters_mut(&mut self) -> Option<&mut CountersBlock> {
        self.bkp_counters.as_mut()
    }
}

fn collect_counters<'f>(
    elf: &'f File,
    filter_section_idx: SectionIndex,
    raw_symbols: &mut Vec<&'f str>,
    dedup_str: &mut Vec<Arc<String>>,
) -> BTreeMap<u64, Counter> {
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
        counters.insert(
            symbol_addr,
            Counter {
                group,
                crate_name,
                package,
                name: symbol.name,
                storage: symbol.storage,
                ty,
                severity: symbol.severity,
                location: None,
                buf: Buffer { addr: 0, size: 0 },
            },
        );
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

fn fill_addresses(counters: &mut BTreeMap<u64, Counter>, buffer: Buffer) {
    for (idx, counter) in counters.iter_mut() {
        counter.buf = Buffer {
            addr: buffer.addr + idx * counter.ty.len() as u64,
            size: counter.ty.len() as u64,
        };
    }
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
