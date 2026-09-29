//! Host side of the `cnt` crate: loads counter descriptions from a firmware ELF and decodes buffer contents.
//!
//! Counters are described by marker symbols the `cnt` macros emit into non-allocated sections placed at address 0 by
//! `cnt.x`, see [`load`] for the format.

mod load;

use anyhow::anyhow;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

pub struct Counters {
    ram_counters: Option<CountersBlock>,
    bkp_counters: Option<CountersBlock>,
}

/// All counters in one buffer (RAM or BKP), keyed by the index of their first word.
pub struct CountersBlock {
    entries: BTreeMap<u64, Counter>,
    buffer: Buffer,
    storage: Storage,
    values: BTreeMap<u64, Value>,
}

pub struct Counter {
    /// Cargo package the counter was declared in. For instance counters: the package that created the instance.
    pub package: Arc<String>,
    /// Crate the counter was declared in, differs from `package` e.g. for a binary in a library package.
    pub crate_name: Arc<String>,
    /// `cnt!` group, or the instance name of a `counters!` instance. Empty if none.
    pub group: Arc<String>,
    /// Counter name, or the variant name of a `Count` enum.
    pub name: String,
    pub ty: Ty,
    pub unit: String,
    pub severity: Severity,
    /// `Count` type an instance counter belongs to, `None` for `cnt!` counters.
    pub layout: Option<Arc<String>>,
    /// Where the counter was declared: the `cnt!` invocation, or the `counters!` invocation of the instance.
    pub location: Location,
    /// Index of the first 32-bit word in the buffer.
    pub idx: u64,
    /// Address of the counter in target memory.
    pub addr: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize)]
pub enum Storage {
    #[serde(rename = "ram")]
    Ram,
    #[serde(rename = "bkp")]
    Bkp,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize)]
pub enum Ty {
    #[serde(rename = "u32")]
    U32,
    #[serde(rename = "u64")]
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

/// Source location of a macro invocation, as seen by the compiler (relative to the workspace or remapped, not
/// necessarily an existing path on the host).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub file: Arc<String>,
    pub line: u64,
}

/// Location and size of a buffer in target's memory space
#[derive(Copy, Clone, Debug)]
pub struct Buffer {
    pub addr: u64,
    pub size: u64,
}

impl Display for Location {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

impl Ty {
    /// Size of the type in bytes.
    pub fn bytes(&self) -> u64 {
        match self {
            Ty::U32 => 4,
            Ty::U64 => 8,
        }
    }

    /// Size of the type in 32-bit words.
    pub fn words(&self) -> u64 {
        self.bytes() / 4
    }
}

impl Counters {
    pub fn is_empty(&self) -> bool {
        self.ram_counters.is_none() && self.bkp_counters.is_none()
    }
}

impl CountersBlock {
    /// Counters keyed by the index of their first word.
    pub fn entries(&self) -> &BTreeMap<u64, Counter> {
        &self.entries
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn storage(&self) -> Storage {
        self.storage
    }

    /// Bytes of the buffer taken by counters.
    pub fn used_bytes(&self) -> u64 {
        self.entries.values().map(|c| c.ty.bytes()).sum()
    }

    /// Decode counter values from the contents of the buffer read from the target.
    pub fn read_values(&mut self, buf: &[u8]) -> anyhow::Result<()> {
        if buf.len() as u64 != self.buffer.size {
            return Err(anyhow!(
                "Invalid buffer size: {} B, expected {} B",
                buf.len(),
                self.buffer.size
            ));
        }
        for (idx, counter) in self.entries.iter() {
            // Counters are validated to lie within the buffer when the ELF is loaded, so this only fails on a bug
            let offset = (idx * 4) as usize;
            let bytes = buf
                .get(offset..offset + counter.ty.bytes() as usize)
                .ok_or_else(|| anyhow!("Counter {} is outside of the buffer", counter.name))?;
            let value = match counter.ty {
                Ty::U32 => Value::U32(u32::from_le_bytes(bytes.try_into()?)),
                Ty::U64 => Value::U64(u64::from_le_bytes(bytes.try_into()?)),
            };
            self.values.insert(*idx, value);
        }
        Ok(())
    }

    /// Counters that have been read at least once, with their values.
    pub fn values(&self) -> impl Iterator<Item = (&Counter, Value)> {
        self.values
            .iter()
            .filter_map(|(idx, value)| self.entries.get(idx).map(|counter| (counter, *value)))
    }

    /// All counters, with their values if read.
    pub fn values_opt(&self) -> impl Iterator<Item = (&Counter, Option<Value>)> {
        self.entries
            .iter()
            .map(|(idx, counter)| (counter, self.values.get(idx).copied()))
    }
}

impl Counter {
    /// `group / name` for display, or just `name` if the counter has no group.
    pub fn qualified_name(&self) -> String {
        if self.group.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.group, self.name)
        }
    }
}

impl Display for Storage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Storage::Ram => write!(f, "RAM"),
            Storage::Bkp => write!(f, "BKP"),
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
        write!(f, "{}:{}", self.qualified_name(), self.ty)?;
        if !self.unit.is_empty() {
            write!(f, " `{}`", self.unit)?;
        }
        Ok(())
    }
}
