//! Probe and target selection, following the conventions of the probe-rs CLI.

use super::cargo_config;
use crate::theme::theme;
use anstream::eprintln;
use anyhow::{Context, bail};
use clap::Args;
use probe_rs::config::{Registry, RegistryError, TargetSelector};
use probe_rs::probe::list::Lister;
use probe_rs::probe::{DebugProbeInfo, DebugProbeSelector, Probe, WireProtocol};
use probe_rs::{Permissions, Session};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Args)]
#[command(next_help_heading = "Probe Options")]
pub(crate) struct ProbeOptions {
    /// The target chip to attach to (e.g. STM32H533RE). If omitted, taken from a probe-rs runner in
    /// .cargo/config.toml, or auto-detected
    #[arg(long, env = "PROBE_RS_CHIP")]
    chip: Option<String>,

    /// Path to a chip description file (YAML) to use in addition to the built-in targets
    #[arg(long, env = "PROBE_RS_CHIP_DESCRIPTION_PATH")]
    chip_description_path: Option<PathBuf>,

    /// Use this probe, in the form VID:PID[:SERIAL] or VID:PID-INTERFACE[:SERIAL]
    #[arg(long, env = "PROBE_RS_PROBE")]
    probe: Option<DebugProbeSelector>,

    /// Protocol used to connect to the target (swd or jtag)
    #[arg(long, env = "PROBE_RS_PROTOCOL")]
    protocol: Option<WireProtocol>,

    /// Protocol speed in kHz
    #[arg(long, env = "PROBE_RS_SPEED")]
    speed: Option<u32>,

    /// Assert the reset pin while attaching to the target
    #[arg(long, env = "PROBE_RS_CONNECT_UNDER_RESET")]
    connect_under_reset: bool,
}

impl ProbeOptions {
    /// Open the selected probe and attach to the selected target.
    pub fn attach(&self, elf_path: &Path) -> anyhow::Result<Session> {
        let registry = self.registry()?;
        let target = self.target_selector(&registry, elf_path)?;
        let mut probe = self.open_probe()?;

        if let Some(protocol) = self.protocol {
            probe
                .select_protocol(protocol)
                .with_context(|| format!("Failed to select protocol {protocol}"))?;
        }
        if let Some(speed) = self.speed {
            let actual = probe
                .set_speed(speed)
                .with_context(|| format!("Failed to set speed to {speed} kHz"))?;
            if actual != speed {
                let warn = theme().warn;
                eprintln!("⚠️ {warn}Requested {speed} kHz, probe is using {actual} kHz{warn:#}");
            }
        }

        let session = if self.connect_under_reset {
            probe.attach_under_reset_with_registry(target, Permissions::default(), &registry)
        } else {
            probe.attach_with_registry(target, Permissions::default(), &registry)
        };
        session.map_err(|e| {
            let autodetect_failed = matches!(
                e,
                probe_rs::Error::ChipNotFound(RegistryError::ChipAutodetectFailed)
            );
            let e = anyhow::Error::new(e).context("Failed to attach to the target");
            if autodetect_failed {
                e.context(
                    "Unable to detect the target chip, specify it with --chip or PROBE_RS_CHIP",
                )
            } else {
                e
            }
        })
    }

    fn registry(&self) -> anyhow::Result<Registry> {
        let mut registry = Registry::from_builtin_families();
        if let Some(path) = &self.chip_description_path {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            registry
                .add_target_family_from_yaml(&yaml)
                .with_context(|| format!("Failed to load chip description {}", path.display()))?;
        }
        Ok(registry)
    }

    fn target_selector(
        &self,
        registry: &Registry,
        elf_path: &Path,
    ) -> anyhow::Result<TargetSelector> {
        if let Some(chip) = &self.chip {
            return Ok(TargetSelector::Specified(
                registry.get_target_by_name(chip)?,
            ));
        }
        let Some(found) = cargo_config::find_chip(elf_path) else {
            return Ok(TargetSelector::Auto);
        };
        let (hint, path) = (theme().hint, theme().path);
        eprintln!(
            "{hint}Using chip{hint:#} {} {hint}from{hint:#} {path}{}{path:#}",
            found.chip,
            found.path.display()
        );
        let target = registry
            .get_target_by_name(&found.chip)
            .with_context(|| format!("Invalid chip in {}", found.path.display()))?;
        Ok(TargetSelector::Specified(target))
    }

    fn open_probe(&self) -> anyhow::Result<Probe> {
        let lister = Lister::new();
        // Resolve the selector against the connected probes instead of calling `Lister::open` directly: that one picks
        // the first match silently and panics in some probe drivers if nothing matches.
        let probes = lister.list(self.probe.as_ref());
        let info = match probes.len() {
            0 => match &self.probe {
                Some(selector) => bail!("Probe {selector} not found"),
                None => bail!("No debug probes found"),
            },
            1 => &probes[0],
            _ => select_probe(&probes)?,
        };
        info.open()
            .with_context(|| format!("Failed to open probe {info}"))
    }
}

/// Let the user pick one of several connected probes, or fail if not running interactively.
fn select_probe(probes: &[DebugProbeInfo]) -> anyhow::Result<&DebugProbeInfo> {
    let list = probes
        .iter()
        .enumerate()
        .map(|(i, p)| format!("  [{i}]: {p}"))
        .collect::<Vec<_>>()
        .join("\n");

    if !std::io::stdin().is_terminal() {
        bail!("Multiple probes found, select one with --probe VID:PID[:SERIAL]:\n{list}");
    }

    let hint = theme().hint;
    eprintln!("{hint}Available probes:{hint:#}\n{list}");
    let stdin = std::io::stdin();
    loop {
        anstream::eprint!("Select probe (0-{}): ", probes.len() - 1);
        std::io::stderr().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("No probe selected");
        }
        match line.trim().parse::<usize>() {
            Ok(i) if i < probes.len() => return Ok(&probes[i]),
            _ => {
                let warn = theme().warn;
                eprintln!("{warn}Invalid selection{warn:#}");
            }
        }
    }
}
