mod location;
mod load;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use serde::Deserialize;

pub struct Counters {
    ram_counters: BTreeMap<u64, Counter>,
    bkp_counters: BTreeMap<u64, Counter>,
    cnt_ram_buffer: Option<Buffer>,
    cnt_bkp_buffer: Option<Buffer>,
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
    pub fn len(&self) -> usize {
        match self {
            Ty::U32 => 4,
            Ty::U64 => 8,
        }
    }
}