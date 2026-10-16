//! The firmware ELF a device runs, found by its GNU build id.
//!
//! Counter names, types and units are only in the ELF, so decoding a device's words with the ELF of other firmware
//! silently shows wrong names. The linker's build id (`-C link-arg=--build-id=sha1`, kept in flash with a
//! `KEEP(*(.note.gnu.build-id))` line in `memory.x`) ties the two together: the device reports it (read from flash
//! over a probe, or `ww_counters`' `INFO.build_id`), and the ELF is looked up by it with [`resolve`]:
//!
//! 1. an ELF given explicitly is used, and refused if its build id differs (unless forced);
//! 2. `<build id>.elf` in the directories of `WW_ELF_PATH`, then in [`config_dirs`] (the `[elf] dirs` entry of
//!    [`config_path`], e.g. a folder synced between the machines that flash a board and the ones that only talk to
//!    it over USB), then in the ELF directory ([`cache_dir`], `~/.cache/ww/elf` on Linux), filled by [`add`] from
//!    single ELFs or release archives;
//! 3. the binaries under the cargo project's `target/` directory, the one with that build id.
//!
//! A device that reports no build id (firmware without the note) gets the newest binary under `target/`, with a
//! warning that it can't be checked.

use crate::Buffer;
use anyhow::{Context, anyhow, bail};
use object::{File, Object, ObjectSection, ObjectSymbol, SectionFlags};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Environment variable with extra directories (`:`-separated, `;` on Windows) holding `<build id>.elf` files,
/// searched before [`config_dirs`] and the ELF directory.
pub const ELF_PATH_VAR: &str = "WW_ELF_PATH";

/// The config file read by [`config_dirs`], `~/.config/ww/config.toml` on Linux: shared by `cnt` and `ww`, like
/// [`cache_dir`].
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ww").join("config.toml"))
}

#[derive(serde::Deserialize, Default)]
struct Config {
    #[serde(default)]
    elf: ElfConfig,
}

#[derive(serde::Deserialize, Default)]
struct ElfConfig {
    /// A leading `~/` is the home directory, so the same line works on every machine regardless of its username.
    ///
    /// ```toml
    /// [elf]
    /// dirs = ["~/sync/ww-elf"]
    /// ```
    #[serde(default)]
    dirs: Vec<PathBuf>,
}

/// Extra directories for `<build id>.elf`, from the `[elf] dirs` entry of [`config_path`]: a persistent setting
/// for a directory that isn't the machine-local [`cache_dir`], e.g. one kept in sync with other machines by
/// Syncthing or similar, so every machine that runs `cnt`/`ww`/a WireWeaver host app finds the same ELFs without
/// anyone exporting an environment variable. Missing file or bad TOML: empty, same as not configured.
pub fn config_dirs() -> Vec<PathBuf> {
    let Some(path) = config_path() else {
        return vec![];
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    parse_config_dirs(&content)
        .into_iter()
        .map(expand_home)
        .collect()
}

/// The `[elf] dirs` entry of a config file's content, empty on bad TOML, `~` not yet expanded.
fn parse_config_dirs(content: &str) -> Vec<PathBuf> {
    toml::from_str::<Config>(content)
        .map(|c| c.elf.dirs)
        .unwrap_or_default()
}

/// A leading `~` or `~/...` replaced with the home directory; any other path unchanged.
fn expand_home(dir: PathBuf) -> PathBuf {
    let Ok(rest) = dir.strip_prefix("~") else {
        return dir;
    };
    dirs::home_dir().map(|home| home.join(rest)).unwrap_or(dir)
}

/// GNU build id of an ELF, `None` if it was linked without one.
pub fn build_id(elf_bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let elf = File::parse(elf_bytes).context("Not an ELF file")?;
    Ok(build_id_of(&elf))
}

pub(crate) fn build_id_of(elf: &File) -> Option<Vec<u8>> {
    elf.build_id()
        .ok()
        .flatten()
        .filter(|id| !id.is_empty())
        .map(<[u8]>::to_vec)
}

/// Where the build id note is in target memory: between the `__build_id_start` and `__build_id_end` symbols the
/// firmware's `memory.x` defines, or the `.note.gnu.build-id` section if it is allocated.
pub(crate) fn note_location(elf: &File) -> Option<Buffer> {
    let symbol = |name: &str| {
        elf.symbols()
            .find(|s| s.name() == Ok(name))
            .map(|s| s.address())
    };
    if let (Some(start), Some(end)) = (symbol("__build_id_start"), symbol("__build_id_end"))
        && end > start
    {
        return Some(Buffer {
            addr: start,
            size: end - start,
        });
    }
    let section = elf.section_by_name(".note.gnu.build-id")?;
    let allocated = matches!(section.flags(), SectionFlags::Elf { sh_flags, .. }
        if sh_flags.0 & object::elf::SHF_ALLOC.0 != 0);
    (allocated && section.address() != 0).then(|| Buffer {
        addr: section.address(),
        size: section.size(),
    })
}

/// The build id in a GNU build id note (`.note.gnu.build-id` contents, read from a device), `None` if the bytes are
/// not one: e.g. a device running other firmware that keeps something else at that address.
pub fn parse_build_id_note(note: &[u8]) -> Option<&[u8]> {
    let word = |i: usize| -> Option<usize> {
        Some(u32::from_le_bytes(note.get(i..i + 4)?.try_into().ok()?) as usize)
    };
    let (name_len, desc_len, kind) = (word(0)?, word(4)?, word(8)?);
    const NT_GNU_BUILD_ID: usize = 3;
    if kind != NT_GNU_BUILD_ID || name_len != 4 || note.get(12..16)? != b"GNU\0" {
        return None;
    }
    // Name padded to 4 bytes
    let desc = note.get(16..16 + desc_len)?;
    (!desc.is_empty()).then_some(desc)
}

/// Lower-case hex of a build id, as in file names of the ELF directory.
pub fn hex(id: &[u8]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// The ELF directory, `~/.cache/ww/elf` on Linux: `<build id>.elf` files, shared by `cnt` and `ww`.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("ww").join("elf"))
}

/// Directories searched for `<build id>.elf`: those in `WW_ELF_PATH`, then [`config_dirs`], then [`cache_dir`].
pub fn elf_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os(ELF_PATH_VAR)
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| !d.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default();
    dirs.extend(config_dirs());
    dirs.extend(cache_dir());
    dirs
}

/// Where [`add`] puts an ELF when the caller doesn't ask for a specific directory: the first of [`config_dirs`]
/// if one is set, so a flash on any machine publishes straight into the shared directory everyone else already
/// searches; [`cache_dir`] otherwise.
pub fn default_write_dir() -> Option<PathBuf> {
    config_dirs().into_iter().next().or_else(cache_dir)
}

/// `<build id>.elf` in one of `dirs`, if its build id really is `id`.
pub fn find_in_dirs(id: &[u8], dirs: &[PathBuf]) -> Option<PathBuf> {
    let name = format!("{}.elf", hex(id));
    dirs.iter()
        .map(|d| d.join(&name))
        .find(|p| has_build_id(p, id))
}

/// The ELF of the firmware with build id `id`: in [`elf_dirs`], then among the [`target_binaries`].
pub fn find(id: &[u8]) -> Option<PathBuf> {
    find_in_dirs(id, &elf_dirs()).or_else(|| {
        target_binaries()
            .ok()?
            .into_iter()
            .find(|p| has_build_id(p, id))
    })
}

fn has_build_id(path: &Path, id: &[u8]) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| build_id(&bytes).ok().flatten())
        .is_some_and(|b| b == id)
}

/// The ELF chosen by [`resolve`].
#[derive(Debug)]
pub struct Resolved {
    pub path: PathBuf,
    pub how: How,
    /// Something to tell the user: the ELF could not be checked, or was used although it doesn't match
    pub warning: Option<String>,
}

/// How [`resolve`] chose the ELF.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum How {
    /// Given explicitly
    Given,
    /// Found by the device's build id
    BuildId,
    /// The newest binary under `target/`, unchecked
    Newest,
}

/// The ELF to decode a device's counters with, see the [module](self) docs for the order.
///
/// `given` is an ELF passed explicitly, `device_id` the build id the device reports (`None` or empty when it has
/// none). A mismatch is an error unless `force`, then a warning.
pub fn resolve(
    given: Option<&Path>,
    device_id: Option<&[u8]>,
    force: bool,
) -> anyhow::Result<Resolved> {
    let device_id = device_id.filter(|id| !id.is_empty());
    let unchecked = "the device reports no build id, so the ELF can't be checked against it (keep the \
                     .note.gnu.build-id section in flash)";
    if let Some(path) = given {
        let bytes =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let elf_id = build_id(&bytes).with_context(|| path.display().to_string())?;
        let warning = match (device_id, elf_id) {
            (None, _) => Some(unchecked.to_string()),
            (Some(_), None) => Some(format!(
                "{} has no build id, it can't be checked against the device's",
                path.display()
            )),
            (Some(dev), Some(elf)) if dev == elf.as_slice() => None,
            (Some(dev), Some(elf)) => {
                let msg = mismatch(path, &elf, dev);
                if !force {
                    bail!("{msg}. Pass the matching ELF, or --force to use this one anyway");
                }
                Some(format!("{msg}, used anyway (--force)"))
            }
        };
        return Ok(Resolved {
            path: path.to_path_buf(),
            how: How::Given,
            warning,
        });
    }
    let Some(id) = device_id else {
        return Ok(Resolved {
            path: newest_target_binary()?,
            how: How::Newest,
            warning: Some(unchecked.to_string()),
        });
    };
    if let Some(path) = find(id) {
        return Ok(Resolved {
            path,
            how: How::BuildId,
            warning: None,
        });
    }
    let searched: Vec<String> = elf_dirs()
        .iter()
        .map(|d| d.display().to_string())
        .chain(["target/ of the cargo project".to_string()])
        .collect();
    let msg = format!(
        "No ELF with the device's build id {} in {}",
        hex(id),
        searched.join(", ")
    );
    if !force {
        bail!(
            "{msg}. Pass the ELF, add it (or the firmware's release ELF archive) with `cnt elf add`, or --force to \
             use the newest one under target/"
        );
    }
    Ok(Resolved {
        path: newest_target_binary()?,
        how: How::Newest,
        warning: Some(format!(
            "{msg}, using the newest one under target/ (--force)"
        )),
    })
}

/// Error message for an ELF that is not the firmware the device runs.
pub fn mismatch(path: &Path, elf_id: &[u8], device_id: &[u8]) -> String {
    format!(
        "{} has build id {}, the device runs {}: counters would be decoded with the wrong names",
        path.display(),
        hex(elf_id),
        hex(device_id)
    )
}

/// An ELF [`add`] put into the ELF directory.
#[derive(Debug)]
pub struct Added {
    pub build_id: Vec<u8>,
    /// Where it was read from: the file, or `archive:entry`
    pub from: String,
    pub path: PathBuf,
}

/// Put an ELF, or every ELF in a release archive (`.tar.zst` or `.tar`), into `dir` as `<build id>.elf`. ELFs
/// without a build id are skipped (`skipped` names them). Files are written to a temporary name and renamed, so a
/// lookup never sees half of one.
pub fn add(path: &Path, dir: &Path) -> anyhow::Result<(Vec<Added>, Vec<String>)> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let mut added = vec![];
    let mut skipped = vec![];
    let mut put = |name: String, elf: &[u8]| -> anyhow::Result<()> {
        match build_id(elf).with_context(|| name.clone())? {
            Some(id) => {
                let target = dir.join(format!("{}.elf", hex(&id)));
                let tmp = dir.join(format!(".{}.elf.tmp", hex(&id)));
                std::fs::write(&tmp, elf)
                    .with_context(|| format!("Failed to write {}", tmp.display()))?;
                std::fs::rename(&tmp, &target)
                    .with_context(|| format!("Failed to write {}", target.display()))?;
                added.push(Added {
                    build_id: id,
                    from: name,
                    path: target,
                });
            }
            None => skipped.push(name),
        }
        Ok(())
    };
    if bytes.starts_with(ELF_MAGIC) {
        put(path.display().to_string(), &bytes)?;
        return Ok((added, skipped));
    }
    let tar = if bytes.starts_with(ZSTD_MAGIC) {
        let mut tar = vec![];
        ruzstd::decoding::StreamingDecoder::new(bytes.as_slice())
            .map_err(|e| anyhow!("{}: {e}", path.display()))?
            .read_to_end(&mut tar)
            .with_context(|| format!("Failed to decompress {}", path.display()))?;
        tar
    } else {
        bytes
    };
    let mut archive = tar::Archive::new(tar.as_slice());
    let entries = archive
        .entries()
        .with_context(|| format!("{} is neither an ELF nor a tar archive", path.display()))?;
    for entry in entries {
        let mut entry = entry.with_context(|| format!("Failed to read {}", path.display()))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = format!("{}:{}", path.display(), entry.path()?.display());
        let mut data = vec![];
        entry
            .read_to_end(&mut data)
            .with_context(|| format!("Failed to read {name}"))?;
        // An index or notes next to the ELFs
        if data.starts_with(ELF_MAGIC) {
            put(name, &data)?;
        }
    }
    if added.is_empty() && skipped.is_empty() {
        bail!("No ELF files in {}", path.display());
    }
    Ok((added, skipped))
}

const ELF_MAGIC: &[u8] = b"\x7fELF";
const ZSTD_MAGIC: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd];

/// The most recently built binary of the cargo project in the current directory, see [`target_binaries`].
pub fn newest_target_binary() -> anyhow::Result<PathBuf> {
    target_binaries()?
        .into_iter()
        .next()
        .context("No built binary found under target/, build the firmware first or pass the ELF")
}

/// Built binaries of the cargo project in the current directory, newest first.
///
/// If the current directory is inside a workspace member, only its binaries are considered, otherwise binaries of all
/// workspace members. Binaries are searched in `<target dir>/<triple>/<profile>/`.
pub fn target_binaries() -> anyhow::Result<Vec<PathBuf>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("Failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "No ELF given and cargo metadata failed, are you inside a firmware project?\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse cargo metadata")?;

    let target_dir = metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .context("cargo metadata did not return a target directory")?;
    let bins = bin_names(&metadata);
    if bins.is_empty() {
        bail!("No binary targets found in the cargo project");
    }
    Ok(binaries(&target_dir, &bins))
}

/// Names of the binary targets of the package containing the current directory, or of all workspace members.
fn bin_names(metadata: &serde_json::Value) -> Vec<String> {
    let members: Vec<&str> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str())
        .collect();
    let packages: Vec<&serde_json::Value> = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(&id)))
        .collect();

    let package_dir = |p: &serde_json::Value| {
        p["manifest_path"]
            .as_str()
            .and_then(|m| Path::new(m).parent().map(Path::to_path_buf))
    };
    let current = std::env::current_dir().ok().and_then(|cwd| {
        packages
            .iter()
            .filter_map(|p| Some((*p, package_dir(p)?)))
            .filter(|(_, dir)| cwd.starts_with(dir))
            .max_by_key(|(_, dir)| dir.components().count())
            .map(|(p, _)| p)
    });
    let packages = match current {
        Some(p) => vec![p],
        None => packages,
    };

    packages
        .into_iter()
        .flat_map(|p| p["targets"].as_array().into_iter().flatten())
        .filter(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "bin"))
        })
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect()
}

/// `<target_dir>/<triple>/<profile>/<bin>` files, the most recently modified first.
fn binaries(target_dir: &Path, bins: &[String]) -> Vec<PathBuf> {
    let subdirs = |dir: &Path| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect::<Vec<_>>()
    };

    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for triple in subdirs(target_dir) {
        for profile in subdirs(&triple) {
            for bin in bins {
                let path = profile.join(bin);
                if let Ok(modified) = path.metadata().and_then(|m| m.modified())
                    && path.is_file()
                {
                    candidates.push((modified, path));
                }
            }
        }
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    candidates.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/counters_fw.elf");

    #[test]
    fn config_dirs_from_toml() {
        assert_eq!(
            parse_config_dirs("[elf]\ndirs = [\"/a/b\", \"/c/d\"]\n"),
            vec![PathBuf::from("/a/b"), PathBuf::from("/c/d")]
        );
    }

    #[test]
    fn config_dirs_missing_or_bad_is_empty() {
        assert_eq!(parse_config_dirs(""), Vec::<PathBuf>::new());
        assert_eq!(
            parse_config_dirs("not valid toml {{"),
            Vec::<PathBuf>::new()
        );
        assert_eq!(parse_config_dirs("[elf]\n"), Vec::<PathBuf>::new());
    }

    #[test]
    fn home_is_expanded_only_as_a_leading_component() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home(PathBuf::from("~")), home);
        assert_eq!(
            expand_home(PathBuf::from("~/sync/ww-elf")),
            home.join("sync/ww-elf")
        );
        assert_eq!(
            expand_home(PathBuf::from("/already/absolute")),
            PathBuf::from("/already/absolute")
        );
        assert_eq!(
            expand_home(PathBuf::from("~other/sync")),
            PathBuf::from("~other/sync")
        );
    }

    fn note(kind: u32, name: &[u8], desc: &[u8]) -> Vec<u8> {
        let mut n = vec![];
        n.extend((name.len() as u32).to_le_bytes());
        n.extend((desc.len() as u32).to_le_bytes());
        n.extend(kind.to_le_bytes());
        n.extend(name);
        n.extend(desc);
        n
    }

    #[test]
    fn note_parsing() {
        let id = [0xab; 20];
        assert_eq!(parse_build_id_note(&note(3, b"GNU\0", &id)), Some(&id[..]));
        assert_eq!(parse_build_id_note(&note(1, b"GNU\0", &id)), None);
        assert_eq!(parse_build_id_note(&note(3, b"XYZ\0", &id)), None);
        assert_eq!(parse_build_id_note(&note(3, b"GNU\0", &[])), None);
        assert_eq!(parse_build_id_note(&[0xff; 36]), None);
        assert_eq!(parse_build_id_note(&[]), None);
    }

    #[test]
    fn fixture_build_id_and_note_in_flash() {
        let elf = File::parse(FIXTURE).unwrap();
        let id = build_id_of(&elf).unwrap();
        assert_eq!(id.len(), 20);
        let note = note_location(&elf).unwrap();
        assert!(
            note.addr >= 0x0800_0000 && note.addr < 0x0802_0000,
            "{note:?}"
        );
        assert_eq!(note.size, 36);
        // What a probe reads there is the note of the same build id
        let section = elf.section_by_name(".note.gnu.build-id").unwrap();
        assert_eq!(
            parse_build_id_note(section.data().unwrap()),
            Some(id.as_slice())
        );
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cnt_core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_single_elf_and_find_it() {
        let dir = tmp_dir("add-elf");
        let src = dir.join("fw");
        std::fs::write(&src, FIXTURE).unwrap();
        let store = dir.join("store");
        let (added, skipped) = add(&src, &store).unwrap();
        assert!(skipped.is_empty());
        let id = build_id(FIXTURE).unwrap().unwrap();
        assert_eq!(added[0].build_id, id);
        assert_eq!(added[0].path, store.join(format!("{}.elf", hex(&id))));
        assert_eq!(
            find_in_dirs(&id, std::slice::from_ref(&store)),
            Some(added[0].path.clone())
        );
        assert_eq!(find_in_dirs(&[1, 2, 3], &[store]), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn add_release_archive() {
        let dir = tmp_dir("add-archive");
        let mut tar = tar::Builder::new(vec![]);
        let mut header = tar::Header::new_gnu();
        header.set_size(FIXTURE.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "fw-1.0.0/board_a/fw", FIXTURE)
            .unwrap();
        let index = b"index, not an ELF";
        let mut header = tar::Header::new_gnu();
        header.set_size(index.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "fw-1.0.0/index.toml", &index[..])
            .unwrap();
        let tar = tar.into_inner().unwrap();
        let zst = ruzstd::encoding::compress_to_vec(
            tar.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let archive = dir.join("fw-1.0.0-elf.tar.zst");
        std::fs::write(&archive, zst).unwrap();

        let store = dir.join("store");
        let (added, skipped) = add(&archive, &store).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(added.len(), 1);
        assert!(
            added[0].from.ends_with(":fw-1.0.0/board_a/fw"),
            "{}",
            added[0].from
        );
        let id = build_id(FIXTURE).unwrap().unwrap();
        assert_eq!(std::fs::read(&added[0].path).unwrap(), FIXTURE);
        assert_eq!(find_in_dirs(&id, &[store]), Some(added[0].path.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn given_elf_is_checked() {
        let dir = tmp_dir("given");
        let path = dir.join("fw");
        std::fs::write(&path, FIXTURE).unwrap();
        let id = build_id(FIXTURE).unwrap().unwrap();

        let ok = resolve(Some(&path), Some(&id), false).unwrap();
        assert_eq!((ok.how, ok.warning), (How::Given, None));

        let other = [0x55; 20];
        let err = resolve(Some(&path), Some(&other), false).unwrap_err();
        assert!(err.to_string().contains("wrong names"), "{err}");
        let forced = resolve(Some(&path), Some(&other), true).unwrap();
        assert!(forced.warning.unwrap().contains("--force"));

        // Nothing to check against
        let unchecked = resolve(Some(&path), Some(&[]), false).unwrap();
        assert!(unchecked.warning.unwrap().contains("no build id"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
