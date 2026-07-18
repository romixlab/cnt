use std::path::PathBuf;
use clap::{Parser, Subcommand};

/// Command line interface for the embedded counters crate.
/// https://crates.io/crates/cnt
///
#[derive(Parser)]
// #[command(version, about, long_about = None)]
// #[command(propagate_version = true)]
// #[command(color = clap::ColorChoice::Auto)]
pub struct Cli {
    pub elf_path: PathBuf,
}
