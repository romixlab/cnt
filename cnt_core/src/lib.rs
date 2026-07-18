mod location;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use object::{Object, ObjectSection, ObjectSymbol};
use serde::Deserialize;

pub struct Counters {
    entries: BTreeMap<u64, Counter>,
    cnt_ram_buffer_addr: Option<u64>,
    cnt_bkp_buffer_addr: Option<u64>,
}

pub struct Counter {
    pub package: Arc<String>,
    pub crate_name: Arc<String>,
    pub group: Arc<String>,
    pub name: String,
    pub storage: Storage,
    pub ty: Ty,
    pub severity: Severity,
    pub location: Option<Location>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize)]
pub enum Storage {
    #[serde(rename = "cnt_ram")]
    RAM,
    #[serde(rename = "cnt_bkp")]
    BKP,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Ty {
    U32,
    U64
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize)]
pub enum Severity {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "trace")]
    Trace,
}

/// Location of a cnt statement in the elf-file
#[derive(Clone)]
pub struct Location {
    pub file: Arc<PathBuf>,
    pub line: u64,
    pub module: Arc<String>,
}

impl Counters {
    pub fn load_elf(path: &Path) -> Result<Self> {
        let elf_bytes = std::fs::read(path)?;
        let elf = object::File::parse(elf_bytes.as_slice())?;
        let ram_section_idx = elf.section_by_name(".counters_ram").map(|s| s.index());
        let Some(ram_section_idx) = ram_section_idx else {
            return Err(anyhow!("No .counters_ram section found, counting is disabled or cnt.x linker script is not used"));
        };

        let mut raw_symbols = vec![];
        let mut entries = BTreeMap::new();
        let mut dedup_str = vec![];
        let mut cnt_ram_buffer_addr = None;
        let mut cnt_bkp_buffer_addr = None;
        for symbol in elf.symbols() {
            let (Ok(symbol_name), Some(section_idx)) = (symbol.name(), symbol.section_index()) else {
                continue;
            };
            if symbol_name == "_CNT_RAM_BUFFER" {
                cnt_ram_buffer_addr = Some(symbol.address());
                continue;
            }
            if symbol_name == "_CNT_BKP_BUFFER" {
                cnt_bkp_buffer_addr = Some(symbol.address());
                continue;
            }
            if section_idx != ram_section_idx {
                continue;
            }
            if !symbol_name.starts_with("{") {
                continue;
            }
            raw_symbols.push(symbol_name);
            let symbol_addr = symbol.address();
            let symbol: Result<Symbol, _> = serde_json::from_str(&symbol_name);
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
            let group = dedup(&mut dedup_str, symbol.group);
            let package = dedup(&mut dedup_str, symbol.package);
            let crate_name = dedup(&mut dedup_str, symbol.crate_name);
            entries.insert(symbol_addr, Counter {
                group,
                crate_name,
                package,
                name: symbol.name,
                storage: symbol.storage,
                ty,
                severity: symbol.severity,
                location: None,
            });
        }

        let locations = location::get_locations(elf, &raw_symbols)?;
        for (addr, loc) in locations.into_iter() {
            if let Some(entry) = entries.get_mut(&addr) {
                entry.location = Some(loc);
            }
        }

        Ok(Self {
            entries,
            cnt_ram_buffer_addr,
            cnt_bkp_buffer_addr,
        })
    }

    pub fn counters(&self) -> &BTreeMap<u64, Counter> {
        &self.entries
    }
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file.display(), self.line)
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
struct Symbol {
    package: String,
    group: String,
    storage: Storage,
    name: String,
    ty: String,
    severity: Severity,
    // disambiguator: u64,
    crate_name: String,
}