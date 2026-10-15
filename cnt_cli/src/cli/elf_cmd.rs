//! `cnt elf`: the ELF directory, where ELFs are found by the build id of the firmware a target runs.

use anstream::println;
use anyhow::Context;
use clap::{Subcommand, ValueHint};
use cnt_core::elf;
use cnt_view::theme;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum ElfCommand {
    /// Add ELFs to the ELF directory (~/.cache/ww/elf), stored by build id: single ELFs, or a firmware release's ELF
    /// archive (.tar.zst or .tar)
    Add {
        /// ELF files or release archives
        #[arg(required = true, value_hint = ValueHint::FilePath)]
        paths: Vec<PathBuf>,
        /// Directory to add them to instead (e.g. one listed in WW_ELF_PATH)
        #[arg(long, value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Print the build id of an ELF, and whether it is in the ELF directory
    Id {
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },
}

pub(crate) fn run(cmd: ElfCommand) -> anyhow::Result<()> {
    let t = theme();
    let (hint, path_style, warn) = (t.hint, t.path, t.warn);
    match cmd {
        ElfCommand::Add { paths, dir } => {
            let dir = match dir {
                Some(dir) => dir,
                None => {
                    elf::cache_dir().context("No cache directory on this system, pass --dir")?
                }
            };
            for path in paths {
                let (added, skipped) = elf::add(&path, &dir)?;
                for a in added {
                    println!(
                        "{} {hint}from{hint:#} {path_style}{}{path_style:#}",
                        elf::hex(&a.build_id),
                        a.from
                    );
                }
                for s in skipped {
                    println!("⚠️ {warn}{s} has no build id, skipped{warn:#}");
                }
            }
            println!(
                "{hint}in{hint:#} {path_style}{}{path_style:#}",
                dir.display()
            );
        }
        ElfCommand::Id { path } => {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match elf::build_id(&bytes)? {
                Some(id) => {
                    let stored = elf::find_in_dirs(&id, &elf::elf_dirs());
                    match stored {
                        Some(p) => println!(
                            "{} {hint}in{hint:#} {path_style}{}{path_style:#}",
                            elf::hex(&id),
                            p.display()
                        ),
                        None => {
                            println!("{} {hint}(not in the ELF directory){hint:#}", elf::hex(&id))
                        }
                    }
                }
                None => println!("⚠️ {warn}{} has no build id{warn:#}", path.display()),
            }
        }
    }
    Ok(())
}
