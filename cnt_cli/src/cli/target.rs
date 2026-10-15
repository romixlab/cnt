//! Counters read from target memory through a debug probe, and the check that the target runs the ELF's firmware.

use super::logs::DefmtReader;
use anstream::eprintln;
use anyhow::{Context, bail};
use cnt_core::elf::{self, parse_build_id_note};
use cnt_core::{Buffer, Counters, Log, Source, Storage};
use cnt_view::theme;
use probe_rs::{Core, MemoryInterface};
use std::path::{Path, PathBuf};

/// A [`Source`] reading the buffers at their addresses in the ELF, with the defmt logs over RTT.
pub struct ProbeSource<'p> {
    core: Core<'p>,
    ram: Option<Buffer>,
    bkp: Option<Buffer>,
    defmt: Option<DefmtReader>,
}

impl<'p> ProbeSource<'p> {
    pub fn new(core: Core<'p>, counters: &Counters, defmt: Option<DefmtReader>) -> Self {
        ProbeSource {
            core,
            ram: counters.ram_counters().map(|b| *b.buffer()),
            bkp: counters.bkp_counters().map(|b| *b.buffer()),
            defmt,
        }
    }

    fn buffer(&self, storage: Storage) -> anyhow::Result<Buffer> {
        match storage {
            Storage::Ram => self.ram,
            Storage::Bkp => self.bkp,
        }
        .with_context(|| format!("The ELF has no {storage} counters"))
    }
}

impl Source for ProbeSource<'_> {
    fn read(&mut self, storage: Storage, offset: u64, words: &mut [u32]) -> anyhow::Result<()> {
        let buffer = self.buffer(storage)?;
        self.core.read_32(buffer.addr + offset * 4, words)?;
        Ok(())
    }

    fn reset(&mut self, storage: Storage) -> anyhow::Result<()> {
        let buffer = self.buffer(storage)?;
        let zeros = vec![0u32; (buffer.size / 4) as usize];
        self.core.write_32(buffer.addr, &zeros)?;
        Ok(())
    }

    fn poll_logs(&mut self) -> anyhow::Result<Vec<Log>> {
        match &mut self.defmt {
            Some(defmt) => defmt.poll(&mut self.core),
            None => Ok(Vec::new()),
        }
    }

    fn logs_attached(&self) -> Option<bool> {
        self.defmt.as_ref().map(DefmtReader::is_attached)
    }

    fn close(&mut self) -> anyhow::Result<()> {
        match &mut self.defmt {
            Some(defmt) => defmt.detach(&mut self.core),
            None => Ok(()),
        }
    }
}

/// Check that the target runs the firmware of the ELF, by reading the build id note from its flash where the ELF
/// keeps it.
///
/// A mismatch is an error (the counters would be decoded with the wrong names) unless `force`. When the ELF was not
/// given explicitly and one with the target's build id is found (see [`cnt_core::elf`]), that one is returned to be
/// used instead.
pub fn check_firmware(
    core: &mut Core,
    counters: &Counters,
    elf_path: &Path,
    given: bool,
    force: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let warn = theme().warn;
    let (Some(elf_id), Some(note)) = (counters.build_id(), counters.build_id_note()) else {
        eprintln!(
            "⚠️ {warn}{} keeps no build id in flash, it can't be checked against the target's firmware (link with \
             --build-id and KEEP(*(.note.gnu.build-id)) in memory.x){warn:#}",
            elf_path.display()
        );
        return Ok(None);
    };
    let mut bytes = vec![0u8; note.size as usize];
    core.read(note.addr, &mut bytes)
        .context("Failed to read the build id from the target")?;
    let msg = match parse_build_id_note(&bytes) {
        Some(id) if id == elf_id => return Ok(None),
        Some(id) => {
            if !given && let Some(found) = elf::find(id) {
                let (hint, path) = (theme().hint, theme().path);
                eprintln!(
                    "{hint}Using ELF{hint:#} {path}{}{path:#} {hint}with the target's build id {}{hint:#}",
                    found.display(),
                    elf::hex(id)
                );
                return Ok(Some(found));
            }
            elf::mismatch(elf_path, elf_id, id)
        }
        None => format!(
            "The target has no build id at 0x{:08x}, where {} keeps it: it runs other firmware, or one linked \
             without the note",
            note.addr,
            elf_path.display()
        ),
    };
    if force {
        eprintln!("⚠️ {warn}{msg}, used anyway (--force){warn:#}");
        return Ok(None);
    }
    bail!(
        "{msg}. Pass the ELF of the firmware the target runs, add it with `cnt elf add`, or --force to use this one \
         anyway"
    )
}
