//! Locate the firmware ELF when it is not given explicitly.

use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Find the most recently built binary of the cargo project in the current directory.
///
/// If the current directory is inside a workspace member, only its binaries are considered, otherwise binaries of all
/// workspace members. Binaries are searched in `<target dir>/<triple>/<profile>/`.
pub fn find_elf() -> anyhow::Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("Failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "No ELF_PATH given and cargo metadata failed, are you inside a firmware project?\n{}",
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

    newest_binary(&target_dir, &bins).with_context(|| {
        format!(
            "No built binary ({}) found in {}, build the firmware first or pass ELF_PATH",
            bins.join(", "),
            target_dir.display()
        )
    })
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

/// The most recently modified `<target_dir>/<triple>/<profile>/<bin>` file.
fn newest_binary(target_dir: &Path, bins: &[String]) -> Option<PathBuf> {
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
    candidates
        .into_iter()
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}
