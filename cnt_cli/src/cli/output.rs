//! Machine readable output (`--format json` and `--format jsonl`), for scripts, CI and agents.
//!
//! - `json`: one pretty-printed document, `{"elf": ..., "blocks": [{"storage": "ram", ..., "counters": [...]}]}`.
//! - `jsonl`: one compact object per counter and line, with the block's `storage` added to each.
//!
//! Counters have a `value` member only when read from a target. `read --watch` prints `jsonl` lines with an `event`
//! (see [`Event`]) and a `ts` (Unix time in milliseconds) in front of the counter. defmt logs are interleaved as
//! `{"event": "log", "ts": ..., "level": ..., "timestamp": ..., "message": ..., "module": ..., "file": ..., "line": ...}`,
//! `level`, `timestamp` and the location are `null` if not available.
//! Diagnostics go to stderr, stdout only has JSON.

use super::logs::Log;
use clap::ValueEnum;
use cnt_core::{Counter, Counters, CountersBlock, Storage, Value};
use serde_json::{Value as Json, json};
use std::io::{self, Write};
use std::path::Path;

#[derive(Copy, Clone, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum Format {
    /// Human readable, coloured if the terminal supports it
    #[default]
    Text,
    /// A single JSON document
    Json,
    /// One JSON object per counter and line
    Jsonl,
}

/// Print all counters, with their values if they were read.
pub fn counters(format: Format, counters: &Counters, elf_path: &Path) -> anyhow::Result<()> {
    match format {
        Format::Text => unreachable!("text output is printed by the commands themselves"),
        Format::Json => {
            let blocks: Vec<Json> = counters.blocks().map(block).collect();
            write_pretty(&json!({
                "elf": elf_path.display().to_string(),
                "blocks": blocks,
            }))
        }
        Format::Jsonl => write_lines(counters.blocks().flat_map(|b| {
            b.values_opt().map(|(c, value)| {
                let mut line = json!({ "storage": b.storage() });
                merge(&mut line, counter(c, value));
                line
            })
        })),
    }
}

/// What a `read --watch` line reports, the value is always the current one.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Event {
    /// First read, every counter is printed once
    Initial,
    /// Value increased since the previous read
    Change,
    /// All counters of a buffer that were non-zero decreased: the target or the counters were reset
    Reset,
    /// Value decreased while other counters of the same buffer did not. Counters saturate rather than wrap, so this
    /// is the firmware writing the buffer, a torn read of a `u64` counter whose low word just overflowed, or corruption
    Decrease,
}

impl Event {
    pub fn as_str(&self) -> &'static str {
        match self {
            Event::Initial => "initial",
            Event::Change => "change",
            Event::Reset => "reset",
            Event::Decrease => "decrease",
        }
    }
}

/// One `jsonl` line of `read --watch`.
pub fn watch_event(
    event: Event,
    ts: u64,
    storage: Storage,
    c: &Counter,
    value: Value,
) -> anyhow::Result<()> {
    let mut line = json!({ "event": event.as_str(), "ts": ts, "storage": storage });
    merge(&mut line, counter(c, Some(value)));
    write_lines(std::iter::once(line))
}

/// One `jsonl` line of a defmt log in watch mode.
pub fn log_event(ts: u64, log: &Log) -> anyhow::Result<()> {
    let line = json!({
        "event": "log",
        "ts": ts,
        "level": log.severity(),
        "timestamp": log.timestamp,
        "message": log.message,
        "module": log.location.map(|l| &l.module),
        "file": log.location.map(|l| l.file.display().to_string()),
        "line": log.location.map(|l| l.line),
    });
    write_lines(std::iter::once(line))
}

/// Report the buffers that were reset.
pub fn reset(format: Format, storages: &[Storage]) -> anyhow::Result<()> {
    match format {
        Format::Text => unreachable!("text output is printed by the command itself"),
        Format::Json => write_pretty(&json!({ "reset": storages })),
        Format::Jsonl => write_lines(storages.iter().map(|s| json!({ "reset": s }))),
    }
}

fn block(b: &CountersBlock) -> Json {
    let (size, used) = (b.buffer().size, b.used_bytes());
    let counters: Vec<Json> = b.values_opt().map(|(c, v)| counter(c, v)).collect();
    json!({
        "storage": b.storage(),
        "addr": b.buffer().addr,
        "size": size,
        "used": used,
        "free": size as i64 - used as i64,
        "counters": counters,
    })
}

fn counter(c: &Counter, value: Option<Value>) -> Json {
    let mut obj = json!({
        "qualified_name": c.qualified_name(),
        "group": *c.group,
        "name": c.name,
        "ty": c.ty,
        "unit": c.unit,
        "severity": c.severity,
        "package": *c.package,
        "crate": *c.crate_name,
        "layout": c.layout.as_deref(),
        "file": *c.location.file,
        "line": c.location.line,
        "idx": c.idx,
        "addr": c.addr,
    });
    if let Some(value) = value {
        obj["value"] = value.to_u64().into();
    }
    obj
}

fn merge(into: &mut Json, from: Json) {
    if let (Json::Object(into), Json::Object(from)) = (into, from) {
        into.extend(from);
    }
}

fn write_pretty(v: &Json) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    ignore_broken_pipe(
        serde_json::to_writer_pretty(&mut out, v)
            .map_err(io::Error::from)
            .and_then(|_| writeln!(out)),
    )
}

fn write_lines(lines: impl Iterator<Item = Json>) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in lines {
        ignore_broken_pipe(
            serde_json::to_writer(&mut out, &line)
                .map_err(io::Error::from)
                .and_then(|_| writeln!(out)),
        )?;
    }
    Ok(())
}

/// Stop quietly when the reader goes away, e.g. `cnt list --format jsonl | head`.
fn ignore_broken_pipe(r: io::Result<()>) -> anyhow::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => std::process::exit(0),
        r => Ok(r?),
    }
}
