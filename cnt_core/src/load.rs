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
        let ram_section_idx = counters_section(&elf, ".counters_ram")?;
        let bkp_section_idx = counters_section(&elf, ".counters_bkp")?;
        if ram_section_idx.is_none() && bkp_section_idx.is_none() {
            return Err(anyhow!(
                "No .counters_ram or .counters_bkp section found, cnt crate or cnt.x linker script is not used"
            ));
        };

        let mut ram_buffer = None;
        let mut bkp_buffer = None;
        let mut signature = None;
        for symbol in elf.symbols() {
            let Ok(symbol_name) = symbol.name() else {
                continue;
            };
            let buffer = Buffer {
                addr: symbol.address(),
                size: symbol.size(),
            };
            match symbol_name {
                "_CNT_RAM_BUFFER" => ram_buffer = Some(buffer),
                "_CNT_BKP_BUFFER" => bkp_buffer = Some(buffer),
                "_CNT_SIGNATURE" => signature = Some(symbol),
                _ => {}
            }
        }
        check_signature(&elf, signature)?;

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
            fill_addresses(&mut ram_counters, ram_buffer, Storage::RAM)?;
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
            fill_addresses(&mut bkp_counters, bkp_buffer, Storage::BKP)?;
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

    /// RAM and BKP counters, whichever are in use.
    pub fn blocks_mut(&mut self) -> impl Iterator<Item = &mut CountersBlock> {
        self.ram_counters
            .iter_mut()
            .chain(self.bkp_counters.iter_mut())
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
        let symbol: anyhow::Result<Symbol, _> = serde_json::from_str(symbol_name);
        let Ok(symbol) = symbol else {
            continue;
        };
        let ty = if symbol.ty == "u32" {
            Ty::U32
        } else if symbol.ty == "u64" {
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
                unit: symbol.unit,
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

/// Index of the `.counters_ram`/`.counters_bkp` section, if present. Counter indices are symbol addresses in this
/// section, which only works if the linker script placed it at address 0.
fn counters_section(elf: &File, name: &str) -> anyhow::Result<Option<SectionIndex>> {
    let Some(section) = elf.section_by_name(name) else {
        return Ok(None);
    };
    if section.address() != 0 {
        return Err(anyhow!(
            "Section {name} is at 0x{:08x} instead of 0, is the cnt.x linker script used unmodified?",
            section.address()
        ));
    }
    Ok(Some(section.index()))
}

/// Format version understood by this crate, see `_CNT_SIGNATURE` in the `cnt` crate.
const SIGNATURE_MAGIC: &[u8] = b"CNTRS\0";
const SIGNATURE_VERSION: u16 = 1;

/// Verify that the firmware uses a compatible version of the `cnt` crate. Firmware built with cnt 0.2.0 has an
/// all-zero signature, which is accepted as well.
fn check_signature(elf: &File, symbol: Option<object::Symbol>) -> anyhow::Result<()> {
    let Some(symbol) = symbol else {
        return Err(anyhow!(
            "No _CNT_SIGNATURE symbol found, cnt crate is not used"
        ));
    };
    let Some(section_idx) = symbol.section_index() else {
        return Ok(());
    };
    let section = elf.section_by_index(section_idx)?;
    let Ok(Some(bytes)) = section.data_range(symbol.address(), symbol.size()) else {
        return Ok(());
    };
    if bytes.iter().all(|b| *b == 0) {
        return Ok(());
    }
    if !bytes.starts_with(SIGNATURE_MAGIC) || bytes.len() < SIGNATURE_MAGIC.len() + 2 {
        return Err(anyhow!("Invalid _CNT_SIGNATURE: {bytes:02x?}"));
    }
    let version = u16::from_le_bytes([
        bytes[SIGNATURE_MAGIC.len()],
        bytes[SIGNATURE_MAGIC.len() + 1],
    ]);
    if version != SIGNATURE_VERSION {
        return Err(anyhow!(
            "Firmware uses cnt format version {version}, this tool supports version {SIGNATURE_VERSION}, update cnt or the cnt CLI"
        ));
    }
    Ok(())
}

/// Compute the target address of each counter from its index and check that it fits into the buffer.
fn fill_addresses(
    counters: &mut BTreeMap<u64, Counter>,
    buffer: Buffer,
    storage: Storage,
) -> anyhow::Result<()> {
    let mut overflowing = vec![];
    for (idx, counter) in counters.iter_mut() {
        // Indices are in 32-bit words
        let size = counter.ty.len() as u64;
        let offset = idx * 4;
        if offset + size > buffer.size {
            overflowing.push(counter.name.clone());
        }
        counter.buf = Buffer {
            addr: buffer.addr + offset,
            size,
        };
    }
    if !overflowing.is_empty() {
        let used: u64 = counters.values().map(|c| c.ty.len() as u64).sum();
        return Err(anyhow!(
            "{storage} counters do not fit into the buffer of {} B ({used} B needed), increase CNT_{storage}_BUFFER_SIZE_WORDS. Outside of the buffer: {}",
            buffer.size,
            overflowing.join(", ")
        ));
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
    unit: String,
    severity: Severity,
    // disambiguator: u64,
    crate_name: String,
}
