//! Borrowed from defmt
use crate::Location;
use crate::load::dedup;
use anyhow::{bail, ensure};
use gimli::DebuggingInformationEntry;
use object::{File, Object, ObjectSection};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) fn get_locations(
    elf: &File,
    filter_symbols: &[&str],
) -> Result<BTreeMap<u64, Location>, anyhow::Error> {
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

        if cursor.next_dfs()?.is_none() {
            // Empty compilation unit, nothing to look for
            continue;
        }

        // Enclosing namespaces of the current entry, with their (absolute) tree depth
        let mut segments: Vec<(isize, String)> = vec![];
        while let Some(entry) = cursor.next_dfs()? {
            let entry: &DebuggingInformationEntry<_> = entry;
            // Leave namespaces that are not ancestors of this entry
            while segments
                .last()
                .is_some_and(|(depth, _)| *depth >= entry.depth())
            {
                segments.pop();
            }

            if entry.tag() == gimli::constants::DW_TAG_namespace {
                if let Some(name) = entry.attr_value(gimli::constants::DW_AT_name) {
                    let name = dwarf.attr_string(&unit, name)?;
                    segments.push((entry.depth(), name.to_string_lossy().into_owned()));
                }
            } else if entry.tag() == gimli::constants::DW_TAG_variable {
                // Locations are best-effort: a counter without one is still listed, so a single odd DIE must not
                // fail the whole ELF
                let Some((addr, file, line)) =
                    cnt_index_variable(entry, &unit, &dwarf, filter_symbols)?
                else {
                    continue;
                };
                let module = segments
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::");

                let module = dedup(&mut dedup_str, module);
                let file = dedup(&mut dedup_path, file);
                let loc = Location { file, line, module };
                if let Some(old) = map.insert(addr, loc.clone()) {
                    bail!(
                        "BUG in DWARF variable filter: index collision for addr 0x{:08x} (old = {:?}, new = {:?})",
                        addr,
                        old,
                        loc
                    );
                }
            }
        }
    }

    Ok(map)
}

/// If `entry` is a `CNT_INDEX` static of a live counter, returns its index (address), declaring file and line.
///
/// Only errors in reading the DWARF structure itself are propagated, an entry with missing or unusual attributes
/// is skipped.
fn cnt_index_variable<R: gimli::read::Reader<Offset = usize>>(
    entry: &DebuggingInformationEntry<R>,
    unit: &gimli::Unit<R>,
    dwarf: &gimli::Dwarf<R>,
    filter_symbols: &[&str],
) -> Result<Option<(u64, PathBuf, u64)>, anyhow::Error> {
    let mut decl_file = None;
    let mut decl_line = None;
    let mut name = None;
    let mut linkage_name = None;
    let mut location = None;

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

    let (Some(name), Some(linkage_name), Some(file_index), Some(line), Some(loc)) =
        (name, linkage_name, decl_file, decl_line, location)
    else {
        return Ok(None);
    };
    let name = dwarf.string(name)?;
    if name.to_string_lossy()? != "CNT_INDEX" {
        return Ok(None);
    }
    let linkage_name = dwarf.string(linkage_name)?;
    if !filter_symbols.contains(&&*linkage_name.to_string_lossy()?) {
        // this symbol was GC-ed by the linker (but remains in the DWARF info),
        // so we discard it (its `addr` info is also wrong which causes collisions)
        return Ok(None);
    }
    let Some(addr) = exprloc2address(unit.encoding(), &loc) else {
        return Ok(None);
    };
    let Ok(file) = file_index_to_path(file_index, unit, dwarf) else {
        return Ok(None);
    };
    Ok(Some((addr, file, line)))
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

        if !dir.is_absolute()
            && let Some(ref comp_dir) = unit.comp_dir
        {
            p.push(&comp_dir.to_string_lossy()?[..]);
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

/// The address of a static from its `DW_AT_location` expression, `None` if the expression is not a plain address.
fn exprloc2address<R: gimli::read::Reader<Offset = usize>>(
    encoding: gimli::Encoding,
    data: &gimli::Expression<R>,
) -> Option<u64> {
    let mut pc = data.0.clone();
    while pc.len() != 0 {
        // `parse` always consumes at least the opcode, so this terminates even on malformed input
        match gimli::Operation::parse(&mut pc, encoding) {
            Ok(gimli::Operation::Address { address }) => return Some(address),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}
