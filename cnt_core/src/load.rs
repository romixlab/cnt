//! Loading counter descriptions from an ELF file.
//!
//! The `cnt` macros emit marker statics whose symbol names are JSON objects (see [`Symbol`]) into non-allocated
//! sections that `cnt.x` places at address 0:
//!
//! - `.counters_ram`/`.counters_bkp`: one byte per 32-bit word of a counter, so the symbol address is the counter's
//!   index in `_CNT_RAM_BUFFER`/`_CNT_BKP_BUFFER`. Holds `counter` markers of `cnt!` call sites and `slots` markers
//!   of `counters!` instances.
//! - `.cnt_layout`: one `layout` marker per `#[derive(Count)]` type, listing its fields. Its address identifies the
//!   layout.
//! - `.cnt_instance`: one [`InstanceInfo`] per `counters!` invocation, two pointers (resolved by the linker) to the
//!   instance's `slots` marker and to its `layout` marker.

use crate::{Buffer, Counter, Counters, CountersBlock, Location, Severity, Storage, Ty};
use anyhow::{Context, anyhow};
use object::{File, Object, ObjectSection, ObjectSymbol, SectionIndex};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

impl Counters {
    pub fn load_elf(path: &Path) -> anyhow::Result<Self> {
        let elf_bytes = std::fs::read(path)?;
        let elf = File::parse(elf_bytes.as_slice())?;
        let mut loader = Loader::new(&elf)?;
        loader.collect()?;
        let Loader {
            ram_buffer,
            bkp_buffer,
            mut ram,
            mut bkp,
            ..
        } = loader;

        let ram_counters = block(&mut ram, ram_buffer, Storage::Ram)?;
        let bkp_counters = block(&mut bkp, bkp_buffer, Storage::Bkp)?;
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
    pub fn blocks(&self) -> impl Iterator<Item = &CountersBlock> {
        self.ram_counters.iter().chain(self.bkp_counters.iter())
    }

    /// RAM and BKP counters, whichever are in use.
    pub fn blocks_mut(&mut self) -> impl Iterator<Item = &mut CountersBlock> {
        self.ram_counters
            .iter_mut()
            .chain(self.bkp_counters.iter_mut())
    }
}

fn block(
    counters: &mut BTreeMap<u64, Counter>,
    buffer: Option<Buffer>,
    storage: Storage,
) -> anyhow::Result<Option<CountersBlock>> {
    if counters.is_empty() {
        return Ok(None);
    }
    let buffer = buffer.ok_or_else(|| {
        anyhow!(
            "No _CNT_{storage}_BUFFER symbol found, cnt crate or cnt.x linker script is not used"
        )
    })?;
    fill_addresses(counters, buffer, storage)?;
    Ok(Some(CountersBlock {
        entries: std::mem::take(counters),
        buffer,
        storage,
        values: Default::default(),
    }))
}

/// Marker symbol names, as emitted by `cnt_macro::symbol`.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Symbol {
    /// A `cnt!` call site, in `.counters_ram`/`.counters_bkp`.
    Counter {
        group: String,
        #[serde(flatten)]
        field: Field,
        #[serde(flatten)]
        site: CallSite,
    },
    /// Words reserved by a `counters!` instance, in `.counters_ram`/`.counters_bkp`. Carries the same members as the
    /// instance marker, which is the one used.
    Slots {},
    /// A `#[derive(Count)]` type, in `.cnt_layout`. The call site of the derive is not used.
    Layout { ty: String, fields: Vec<Field> },
    /// A `counters!` instance, in `.cnt_instance`, the marker's contents are an [`InstanceInfo`].
    Instance {
        #[serde(flatten)]
        instance: Instance,
    },
}

#[derive(Deserialize)]
struct Field {
    name: String,
    ty: Ty,
    unit: String,
    severity: Severity,
}

#[derive(Deserialize)]
struct Instance {
    name: String,
    storage: Storage,
    #[serde(flatten)]
    site: CallSite,
}

#[derive(Deserialize)]
struct CallSite {
    package: String,
    #[serde(rename = "crate")]
    crate_name: String,
    file: String,
    line: u64,
}

/// Contents of an `.cnt_instance` marker, see `cnt::InstanceInfo`.
struct InstanceInfo {
    /// Index of the instance's first word, i.e. address of its `slots` marker.
    slots: u64,
    /// Address of the instance's `layout` marker.
    layout: u64,
}

struct Layout {
    ty: Arc<String>,
    fields: Vec<Field>,
}

/// Format version understood by this crate, see `_CNT_SIGNATURE` in the `cnt` crate.
const SIGNATURE_MAGIC: &[u8] = b"CNTRS\0";
const SIGNATURE_VERSION: u16 = 2;

struct Loader<'e, 'd> {
    elf: &'e File<'d>,
    ram_section: Option<SectionIndex>,
    bkp_section: Option<SectionIndex>,
    layout_section: Option<SectionIndex>,
    instance_section: Option<SectionIndex>,
    ram_buffer: Option<Buffer>,
    bkp_buffer: Option<Buffer>,
    ram: BTreeMap<u64, Counter>,
    bkp: BTreeMap<u64, Counter>,
    /// Layouts by marker address.
    layouts: HashMap<u64, Layout>,
    /// Instances by the address of their `.cnt_instance` marker, filled from `slots` symbols.
    instances: Vec<(Instance, InstanceInfo)>,
    /// Shared strings (packages, crates, groups, files), so that each is allocated once.
    strings: Vec<Arc<String>>,
}

impl<'e, 'd> Loader<'e, 'd> {
    fn new(elf: &'e File<'d>) -> anyhow::Result<Self> {
        let ram_section = counters_section(elf, ".counters_ram")?;
        let bkp_section = counters_section(elf, ".counters_bkp")?;
        if ram_section.is_none() && bkp_section.is_none() {
            return Err(anyhow!(
                "No .counters_ram or .counters_bkp section found, cnt crate or cnt.x linker script is not used"
            ));
        }
        Ok(Self {
            elf,
            ram_section,
            bkp_section,
            layout_section: counters_section(elf, ".cnt_layout")?,
            instance_section: counters_section(elf, ".cnt_instance")?,
            ram_buffer: None,
            bkp_buffer: None,
            ram: BTreeMap::new(),
            bkp: BTreeMap::new(),
            layouts: HashMap::new(),
            instances: Vec::new(),
            strings: Vec::new(),
        })
    }

    fn collect(&mut self) -> anyhow::Result<()> {
        let mut signature = None;
        for symbol in self.elf.symbols() {
            let Ok(name) = symbol.name() else {
                continue;
            };
            match name {
                "_CNT_RAM_BUFFER" => self.ram_buffer = Some(buffer_of(&symbol)),
                "_CNT_BKP_BUFFER" => self.bkp_buffer = Some(buffer_of(&symbol)),
                "_CNT_SIGNATURE" => signature = Some(symbol),
                _ if name.starts_with('{') => self.marker(&symbol, name)?,
                _ => {}
            }
        }
        check_signature(self.elf, signature)?;
        self.expand_instances()
    }

    /// Record a marker symbol depending on the section it is in.
    fn marker(&mut self, symbol: &object::Symbol, name: &str) -> anyhow::Result<()> {
        let Some(section) = symbol.section_index() else {
            return Ok(());
        };
        let Ok(parsed) = serde_json::from_str::<Symbol>(name) else {
            // Not ours, or from an incompatible version, which the signature check reports
            return Ok(());
        };
        let addr = symbol.address();
        match parsed {
            Symbol::Counter { group, field, site } if self.is_counters_section(section) => {
                let storage = if Some(section) == self.ram_section {
                    Storage::Ram
                } else {
                    Storage::Bkp
                };
                let group = self.intern(group);
                let counter = self.counter(field, &site, group, None, addr);
                self.block_mut(storage).insert(addr, counter);
            }
            Symbol::Layout { ty, fields, .. } if Some(section) == self.layout_section => {
                let ty = self.intern(ty);
                self.layouts.insert(addr, Layout { ty, fields });
            }
            Symbol::Instance { instance } if Some(section) == self.instance_section => {
                let info = self
                    .instance_info(symbol)
                    .with_context(|| format!("Instance {}", instance.name))?;
                self.instances.push((instance, info));
            }
            // Slots only reserve space, the instance marker has all the information
            Symbol::Slots {} => {}
            _ => {}
        }
        Ok(())
    }

    /// Read the two pointers of an `InstanceInfo` from the marker's section contents.
    fn instance_info(&self, symbol: &object::Symbol) -> anyhow::Result<InstanceInfo> {
        let section = self.elf.section_by_index(
            symbol
                .section_index()
                .ok_or_else(|| anyhow!("instance marker has no section"))?,
        )?;
        let ptr: usize = if self.elf.is_64() { 8 } else { 4 };
        let bytes = section
            .data_range(symbol.address(), 2 * ptr as u64)?
            .ok_or_else(|| anyhow!("instance marker outside of the .cnt_instance section"))?;
        let read = |b: &[u8]| -> u64 {
            let mut v = [0u8; 8];
            v[..b.len()].copy_from_slice(b);
            u64::from_le_bytes(v)
        };
        Ok(InstanceInfo {
            slots: read(&bytes[..ptr]),
            layout: read(&bytes[ptr..]),
        })
    }

    /// Create a counter per field of every instance.
    fn expand_instances(&mut self) -> anyhow::Result<()> {
        let instances = std::mem::take(&mut self.instances);
        for (instance, info) in instances {
            let layout = self.layouts.get(&info.layout).ok_or_else(|| {
                anyhow!(
                    "Instance {} refers to an unknown layout at 0x{:x}, is the cnt.x linker script used unmodified?",
                    instance.name,
                    info.layout
                )
            })?;
            let ty = layout.ty.clone();
            // Fields are cloned as they are shared between instances of the same layout
            let fields: Vec<Field> = layout
                .fields
                .iter()
                .map(|f| Field {
                    name: f.name.clone(),
                    ty: f.ty,
                    unit: f.unit.clone(),
                    severity: f.severity,
                })
                .collect();
            let group = self.intern(instance.name.clone());
            let mut idx = info.slots;
            for field in fields {
                let words = field.ty.words();
                let counter =
                    self.counter(field, &instance.site, group.clone(), Some(ty.clone()), idx);
                self.block_mut(instance.storage).insert(idx, counter);
                idx += words;
            }
        }
        Ok(())
    }

    fn counter(
        &mut self,
        field: Field,
        site: &CallSite,
        group: Arc<String>,
        layout: Option<Arc<String>>,
        idx: u64,
    ) -> Counter {
        Counter {
            package: self.intern(site.package.clone()),
            crate_name: self.intern(site.crate_name.clone()),
            group,
            name: field.name,
            ty: field.ty,
            unit: field.unit,
            severity: field.severity,
            layout,
            location: Location {
                file: self.intern(site.file.clone()),
                line: site.line,
            },
            idx,
            // Filled in once the buffer is known
            addr: 0,
        }
    }

    fn is_counters_section(&self, section: SectionIndex) -> bool {
        Some(section) == self.ram_section || Some(section) == self.bkp_section
    }

    fn block_mut(&mut self, storage: Storage) -> &mut BTreeMap<u64, Counter> {
        match storage {
            Storage::Ram => &mut self.ram,
            Storage::Bkp => &mut self.bkp,
        }
    }

    fn intern(&mut self, s: String) -> Arc<String> {
        if let Some(v) = self.strings.iter().find(|v| ***v == s) {
            v.clone()
        } else {
            let v = Arc::new(s);
            self.strings.push(v.clone());
            v
        }
    }
}

fn buffer_of(symbol: &object::Symbol) -> Buffer {
    Buffer {
        addr: symbol.address(),
        size: symbol.size(),
    }
}

/// Index of a marker section, if present. Marker addresses are indices, which only works if the linker script placed
/// the section at address 0.
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

/// Verify that the firmware uses a compatible version of the `cnt` crate.
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
        return Err(anyhow!(
            "Firmware uses cnt 0.2, this tool supports cnt format version {SIGNATURE_VERSION}, update cnt"
        ));
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
        let offset = idx * 4;
        if offset + counter.ty.bytes() > buffer.size {
            overflowing.push(counter.qualified_name());
        }
        counter.addr = buffer.addr + offset;
    }
    if !overflowing.is_empty() {
        let used: u64 = counters.values().map(|c| c.ty.bytes()).sum();
        return Err(anyhow!(
            "{storage} counters do not fit into the buffer of {} B ({used} B needed), increase CNT_{storage}_BUFFER_SIZE_WORDS. Outside of the buffer: {}",
            buffer.size,
            overflowing.join(", ")
        ));
    }
    Ok(())
}
