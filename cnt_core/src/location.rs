//! Borrowed from defmt
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use anyhow::{anyhow, bail, ensure};
use gimli::DebuggingInformationEntry;
use object::{File, Object, ObjectSection};
use crate::load::dedup;
use crate::Location;

pub(crate) fn get_locations(elf: &File, filter_symbols: &[&str]) -> Result<BTreeMap<u64, Location>, anyhow::Error> {
    let endian = if elf.is_little_endian() {
        gimli::RunTimeEndian::Little
    } else {
        gimli::RunTimeEndian::Big
    };

    let load_section = |id: gimli::SectionId| {
        Ok(if let Some(s) = elf.section_by_name(id.name()) {
            s.uncompressed_data().unwrap_or(Cow::Borrowed(&[][..]))
        } else {
            Cow::Borrowed(&[][..])
        })
    };
    let load_section_sup = |_| Ok(Cow::Borrowed(&[][..]));

    let dwarf_sections =
        gimli::DwarfSections::<Cow<[u8]>>::load::<_, anyhow::Error>(&load_section)?;
    let dwarf_sup_sections = gimli::DwarfSections::load::<_, anyhow::Error>(&load_section_sup)?;

    let borrow_section: &dyn for<'a> Fn(
        &'a Cow<[u8]>,
    ) -> gimli::EndianSlice<'a, gimli::RunTimeEndian> =
        &|section| gimli::EndianSlice::new(section, endian);

    let dwarf = dwarf_sections.borrow_with_sup(Some(&dwarf_sup_sections), &borrow_section);

    let mut units = dwarf.debug_info.units();

    let mut map = BTreeMap::new();
    let mut dedup_str = vec![];
    let mut dedup_path = vec![];
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        let abbrev = header.abbreviations(&dwarf.debug_abbrev)?;

        let mut cursor = header.entries(&abbrev);

        ensure!(cursor.next_dfs()?.is_some(), "empty DWARF?");

        let mut segments = vec![];
        let mut depth = 0;
        while let Some(entry) = cursor.next_dfs()? {
            let entry: &DebuggingInformationEntry<_> = entry;
            depth += entry.depth;

            // here start the custom logic
            if entry.tag() == gimli::constants::DW_TAG_namespace {
                for attr in entry.attrs() {
                    if attr.name() == gimli::constants::DW_AT_name {
                        if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                            let s = dwarf.string(off)?;
                            for _ in (depth as usize)..segments.len() + 1 {
                                segments.pop();
                            }
                            segments.push(core::str::from_utf8(&s)?.to_string());
                        }
                    }
                }
            } else if entry.tag() == gimli::constants::DW_TAG_variable {
                // what we are after
                let mut decl_file = None;
                let mut decl_line = None; // line number
                let mut name = None;
                let mut linkage_name = None;
                let mut location = None;

                // Iterate over the attributes in the DIE.
                for attr in entry.attrs() {
                    match attr.name() {
                        gimli::constants::DW_AT_name => {
                            if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                                name = Some(off);
                            }
                        }
                        gimli::constants::DW_AT_decl_file => {
                            if let gimli::AttributeValue::FileIndex(idx) = attr.value() {
                                decl_file = Some(idx);
                            }
                        }
                        gimli::constants::DW_AT_decl_line => {
                            if let gimli::AttributeValue::Udata(line) = attr.value() {
                                decl_line = Some(line);
                            }
                        }
                        gimli::constants::DW_AT_location => {
                            if let gimli::AttributeValue::Exprloc(loc) = attr.value() {
                                location = Some(loc);
                            }
                        }
                        gimli::constants::DW_AT_linkage_name => {
                            if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                                linkage_name = Some(off);
                            }
                        }
                        _ => {}
                    }
                }

                if let (
                    Some(name_index),
                    Some(linkage_name_index),
                    Some(file_index),
                    Some(line),
                    Some(loc),
                ) = (name, linkage_name, decl_file, decl_line, location)
                {
                    let name_slice = dwarf.string(name_index)?;
                    let name = core::str::from_utf8(&name_slice)?;
                    let linkage_name_slice = dwarf.string(linkage_name_index)?;
                    let linkage_name = core::str::from_utf8(&linkage_name_slice)?;

                    if name == "CNT_INDEX" {
                        if filter_symbols.iter().any(|i| *i == linkage_name) {
                            let addr = exprloc2address(unit.encoding(), &loc)?;
                            let file = file_index_to_path(file_index, &unit, &dwarf)?;
                            let module = segments.join("::");

                            let module = dedup(&mut dedup_str, module);
                            let file = dedup(&mut dedup_path, file);
                            let loc = Location { file, line, module };
                            if let Some(old) = map.insert(addr, loc.clone()) {
                                bail!("BUG in DWARF variable filter: index collision for addr 0x{:08x} (old = {:?}, new = {:?})", addr, old, loc);
                            }
                        } else {
                            // this symbol was GC-ed by the linker (but remains in the DWARF info),
                            // so we discard it (its `addr` info is also wrong which causes collisions)
                        }
                    }
                }
            }
        }
    }

    Ok(map)
}

fn file_index_to_path<R>(
    index: u64,
    unit: &gimli::Unit<R>,
    dwarf: &gimli::Dwarf<R>,
) -> Result<PathBuf, anyhow::Error>
where
    R: gimli::read::Reader,
{
    ensure!(index != 0, "`FileIndex` was zero");

    let header = if let Some(program) = &unit.line_program {
        program.header()
    } else {
        bail!("no `LineProgram`");
    };

    let file = if let Some(file) = header.file(index) {
        file
    } else {
        bail!("no `FileEntry` for index {}", index)
    };

    let mut p = PathBuf::new();
    if let Some(dir) = file.directory(header) {
        let dir = dwarf.attr_string(unit, dir)?;
        let dir_s = dir.to_string_lossy()?;
        let dir = Path::new(&dir_s[..]);

        if !dir.is_absolute() {
            if let Some(ref comp_dir) = unit.comp_dir {
                p.push(&comp_dir.to_string_lossy()?[..]);
            }
        }
        p.push(dir);
    }

    p.push(
        &dwarf
            .attr_string(unit, file.path_name())?
            .to_string_lossy()?[..],
    );

    Ok(p)
}

fn exprloc2address<R: gimli::read::Reader<Offset = usize>>(
    encoding: gimli::Encoding,
    data: &gimli::Expression<R>,
) -> Result<u64, anyhow::Error> {
    let mut pc = data.0.clone();
    while pc.len() != 0 {
        if let Ok(gimli::Operation::Address { address }) =
            gimli::Operation::parse(&mut pc, encoding)
        {
            return Ok(address);
        }
    }

    Err(anyhow!("`Operation::Address` not found"))
}
