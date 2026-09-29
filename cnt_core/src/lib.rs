mod load;
mod location;

use anyhow::anyhow;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Counters {
    ram_counters: Option<CountersBlock>,
    bkp_counters: Option<CountersBlock>,
}

pub struct CountersBlock {
    entries: BTreeMap<u64, Counter>,
    buffer: Buffer,
    storage: Storage,
    values: BTreeMap<u64, Value>,
}

pub struct Counter {
    pub package: Arc<String>,
    pub crate_name: Arc<String>,
    pub group: Arc<String>,
    pub name: String,
    pub storage: Storage,
    pub ty: Ty,
    pub unit: String,
    pub severity: Severity,
    pub location: Option<Location>,
    pub buf: Buffer,
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
    U64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Value {
    U32(u32),
    U64(u64),
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

/// Location of a cnt statement in an ELF file
#[derive(Clone)]
pub struct Location {
    pub file: Arc<PathBuf>,
    pub line: u64,
    pub module: Arc<String>,
}

/// Location and size of a buffer in target's memory space
#[derive(Copy, Clone)]
pub struct Buffer {
    pub addr: u64,
    pub size: u64,
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file.display(), self.line)
    }
}

impl Ty {
    /// Returns the size of the type in bytes.
    // A type always has a size, `is_empty` would make no sense
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match self {
            Ty::U32 => 4,
            Ty::U64 => 8,
        }
    }
}

impl Counters {
    pub fn is_empty(&self) -> bool {
        self.ram_counters.is_none() && self.bkp_counters.is_none()
    }
}

impl CountersBlock {
    pub fn entries(&self) -> &BTreeMap<u64, Counter> {
        &self.entries
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn storage(&self) -> Storage {
        self.storage
    }

    pub fn read_values(&mut self, buf: &[u8]) -> anyhow::Result<()> {
        if buf.len() != self.buffer.size as usize {
            return Err(anyhow!("Invalid buffer size"));
        }
        for (addr, counter) in self.entries.iter() {
            let idx = *addr as usize;
            let base = idx * 4;
            let value = match counter.ty {
                Ty::U32 => Value::U32(u32::from_le_bytes(buf[base..base + 4].try_into()?)),
                Ty::U64 => Value::U64(u64::from_le_bytes(buf[base..base + 8].try_into()?)),
            };
            self.values.insert(*addr, value);
        }
        Ok(())
    }

    pub fn values(&self) -> impl Iterator<Item = (&Counter, Value)> {
        self.values
            .iter()
            .filter_map(|(addr, value)| self.entries.get(addr).map(|counter| (counter, *value)))
    }

    pub fn values_opt(&self) -> impl Iterator<Item = (&Counter, Option<Value>)> {
        self.entries.iter().map(|(addr, counter)| {
            let value = self.values.get(addr).cloned();
            (counter, value)
        })
    }
}

impl Display for Storage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Storage::RAM => write!(f, "RAM"),
            Storage::BKP => write!(f, "BKP"),
        }
    }
}

impl Value {
    pub fn to_u64(&self) -> u64 {
        match self {
            Value::U32(v) => *v as u64,
            Value::U64(v) => *v,
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Value::U32(v) => write!(f, "{}", v),
            Value::U64(v) => write!(f, "{}", v),
        }
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Ty::U32 => write!(f, "u32"),
            Ty::U64 => write!(f, "u64"),
        }
    }
}

impl Display for Counter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.name, self.ty)?;
        if !self.unit.is_empty() {
            write!(f, " `{}`", self.unit)?;
        }
        Ok(())
    }
}
