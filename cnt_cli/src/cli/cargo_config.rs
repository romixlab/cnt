//! Look up the target chip from a probe-rs runner in `.cargo/config.toml`, e.g.:
//!
//! ```toml
//! [target.'cfg(all(target_arch = "arm", target_os = "none"))']
//! runner = "probe-rs run --chip STM32H533RE"
//! ```

use super::theme::theme;
use anstream::eprintln;
use std::path::{Path, PathBuf};

/// A chip name found in a cargo config file.
pub struct ConfigChip {
    pub chip: String,
    pub path: PathBuf,
}

/// Search cargo config files for a runner with `--chip`.
///
/// The current directory and its ancestors are searched first, as cargo does. Then the ancestors of the ELF file,
/// including `<ancestor>/<ELF name>/` to cover workspaces where the firmware crate has its own `.cargo/config.toml`.
pub fn find_chip(elf_path: &Path) -> Option<ConfigChip> {
    let elf_path = std::path::absolute(elf_path).unwrap_or_else(|_| elf_path.to_path_buf());
    let elf_name = elf_path.file_stem();

    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.extend(cwd.ancestors().map(Path::to_path_buf));
    }
    for dir in elf_path.ancestors().skip(1) {
        dirs.push(dir.to_path_buf());
        if let Some(name) = elf_name {
            dirs.push(dir.join(name));
        }
    }

    for dir in dirs {
        for file in ["config.toml", "config"] {
            let path = dir.join(".cargo").join(file);
            if let Some(chip) = chip_from_file(&path, &elf_path) {
                return Some(ConfigChip { chip, path });
            }
        }
    }
    None
}

fn chip_from_file(path: &Path, elf_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let config: toml::Table = match content.parse() {
        Ok(config) => config,
        Err(e) => {
            let (warn, style) = (theme().warn, theme().path);
            eprintln!(
                "⚠️ {warn}Failed to parse{warn:#} {style}{}{style:#}{warn}: {e}{warn:#}",
                path.display()
            );
            return None;
        }
    };
    let targets = config.get("target")?.as_table()?;

    // (target key, chip) for every runner that specifies a chip
    let chips: Vec<(&str, String)> = targets
        .iter()
        .filter_map(|(key, target)| Some((key.as_str(), chip_from_runner(target.get("runner")?)?)))
        .collect();

    // A target triple matching the ELF location (target/<triple>/<profile>/) wins over cfg(...) keys
    if let Some((_, chip)) = chips
        .iter()
        .find(|(key, _)| elf_path.components().any(|c| c.as_os_str() == *key))
    {
        return Some(chip.clone());
    }
    let (_, first) = chips.first()?;
    if chips.iter().any(|(_, chip)| chip != first) {
        let (warn, style) = (theme().warn, theme().path);
        eprintln!(
            "⚠️ {warn}Multiple chips found in runners in{warn:#} {style}{}{style:#}{warn}, specify one with --chip{warn:#}",
            path.display()
        );
        return None;
    }
    Some(first.clone())
}

/// Extract the `--chip` argument from a runner, which is either a string or an array of strings.
fn chip_from_runner(runner: &toml::Value) -> Option<String> {
    let args: Vec<&str> = match runner {
        toml::Value::String(s) => s.split_whitespace().collect(),
        toml::Value::Array(a) => a.iter().filter_map(toml::Value::as_str).collect(),
        _ => return None,
    };
    if !args.first()?.ends_with("probe-rs") {
        return None;
    }
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--chip" {
            return args.next().map(str::to_owned);
        }
        if let Some(chip) = arg.strip_prefix("--chip=") {
            return Some(chip.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(runner: &str) -> Option<String> {
        chip_from_runner(&toml::Value::String(runner.into()))
    }

    #[test]
    fn runner_parsing() {
        assert_eq!(
            chip("probe-rs run --chip RP235x").as_deref(),
            Some("RP235x")
        );
        assert_eq!(
            chip("probe-rs run --chip=STM32H533RE").as_deref(),
            Some("STM32H533RE")
        );
        assert_eq!(
            chip("/usr/bin/probe-rs run --connect-under-reset --chip nRF52840_xxAA").as_deref(),
            Some("nRF52840_xxAA")
        );
        assert_eq!(chip("probe-rs run"), None);
        assert_eq!(chip("elf2uf2-rs -d"), None);
        let array = toml::Value::Array(vec![
            "probe-rs".into(),
            "run".into(),
            "--chip".into(),
            "X".into(),
        ]);
        assert_eq!(chip_from_runner(&array).as_deref(), Some("X"));
    }
}
