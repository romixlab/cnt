//! Host side of the `cnt` crate: loads counter descriptions from a firmware ELF and decodes buffer contents.
//!
//! Counters are described by marker symbols the `cnt` macros emit into non-allocated sections placed at address 0 by
//! `cnt.x`, see [`load`] for the format. The buffer words come from a [`Source`]: a debug probe, a WireWeaver device
//! (`ww_counters`) or a test. [`elf`] finds the ELF of the firmware a device runs by its build id.

pub mod elf;
mod load;
mod source;

pub use source::{Change, Event, Source};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

pub struct Counters {
    ram_counters: Option<CountersBlock>,
    bkp_counters: Option<CountersBlock>,
    /// GNU build id of the ELF, `None` if it was linked without one
    build_id: Option<Vec<u8>>,
    /// Where the build id note is in target memory, if the firmware keeps it in flash
    build_id_note: Option<Buffer>,
}

/// All counters in one buffer (RAM or BKP), keyed by the index of their first word.
pub struct CountersBlock {
    entries: BTreeMap<u64, Counter>,
    buffer: Buffer,
    storage: Storage,
    values: BTreeMap<u64, Value>,
}

#[derive(Debug)]
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub enum Storage {
    #[serde(rename = "ram")]
    Ram,
    #[serde(rename = "bkp")]
    Bkp,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
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

/// A log message of the firmware, shown together with the counters (defmt logs over RTT, decoded by the CLI).
#[derive(Clone, Debug)]
pub struct Log {
    /// `None` for `defmt::println!`
    pub severity: Option<Severity>,
    /// Target timestamp, if the firmware defines one
    pub timestamp: Option<String>,
    pub message: String,
    pub location: Option<LogLocation>,
}

/// Where a log statement is in the firmware's source.
#[derive(Clone, Debug)]
pub struct LogLocation {
    pub module: String,
    pub file: std::path::PathBuf,
    pub line: u64,
}

impl Log {
    /// `file:line`, with the file relative to the current directory if it is below it.
    pub fn file_line(&self) -> Option<String> {
        let loc = self.location.as_ref()?;
        let file = std::env::current_dir()
            .ok()
            .and_then(|cwd| {
                loc.file
                    .strip_prefix(cwd)
                    .ok()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| loc.file.clone());
        Some(format!("{}:{}", file.display(), loc.line))
    }
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

    /// GNU build id of the ELF the counters were loaded from, `None` if it was linked without one.
    pub fn build_id(&self) -> Option<&[u8]> {
        self.build_id.as_deref()
    }

    /// Where the firmware keeps its build id note (`.note.gnu.build-id`) in target memory, `None` if it is not in
    /// flash. A probe reads it there to check that the device runs this ELF, see [`elf::parse_build_id_note`].
    pub fn build_id_note(&self) -> Option<Buffer> {
        self.build_id_note
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

    /// Words from the start of the buffer up to the end of the last counter: what a read has to cover. The same as
    /// the `used_words` the firmware reports (`cnt::ram_used_words()`) when it runs this ELF.
    pub fn used_words(&self) -> u64 {
        self.entries
            .iter()
            .next_back()
            .map_or(0, |(idx, c)| idx + c.ty.words())
    }

    /// Size of the buffer in 32-bit words.
    pub fn capacity_words(&self) -> u64 {
        self.buffer.size / 4
    }

    /// Decode counter values from the words of the buffer, read from its start. `words` must cover at least
    /// [`used_words`](Self::used_words).
    pub fn set_words(&mut self, words: &[u32]) -> anyhow::Result<()> {
        if (words.len() as u64) < self.used_words() {
            return Err(anyhow!(
                "{} {} counters words read, {} needed",
                words.len(),
                self.storage,
                self.used_words()
            ));
        }
        for (idx, counter) in self.entries.iter() {
            let idx = *idx as usize;
            let value = match counter.ty {
                Ty::U32 => Value::U32(words[idx]),
                Ty::U64 => Value::U64(u64::from(words[idx + 1]) << 32 | u64::from(words[idx])),
            };
            self.values.insert(idx as u64, value);
        }
        Ok(())
    }

    /// Value of the counter whose first word is `idx`, if read.
    pub fn value(&self, idx: u64) -> Option<Value> {
        self.values.get(&idx).copied()
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
