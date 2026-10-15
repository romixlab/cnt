//! Reading counter words from wherever they come from, and what changed between two reads.

use crate::{Counter, Counters, CountersBlock, Log, Storage, Ty, Value};
use serde::Serialize;
use std::collections::HashMap;

/// Where the words of the counter buffers come from: a debug probe reading target memory, a WireWeaver device
/// serving them (`ww_counters`), a test. Decoding, watching for changes and the views are the same for all.
pub trait Source {
    /// Read `words.len()` words of a buffer, from word `offset`.
    fn read(&mut self, storage: Storage, offset: u64, words: &mut [u32]) -> anyhow::Result<()>;

    /// Set all words of a buffer to 0.
    fn reset(&mut self, storage: Storage) -> anyhow::Result<()>;

    /// Log messages received since the last call, for a source that carries the firmware's logs too.
    fn poll_logs(&mut self) -> anyhow::Result<Vec<Log>> {
        Ok(Vec::new())
    }

    /// `None` when the source carries no logs, `Some(false)` while it does but can't receive them yet (the firmware
    /// hasn't set up RTT).
    fn logs_attached(&self) -> Option<bool> {
        None
    }

    /// Done with the source, also after an error: undo what it changed on the target (the defmt RTT channel mode).
    fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// How often a `u64` counter whose high word keeps changing is re-read before its last value is taken.
const TEAR_RETRIES: usize = 3;

impl Counters {
    /// Read the values of all counters.
    ///
    /// A `u64` counter is two words, an increment carrying into the high word between reading the two gives a torn
    /// value. Counters whose high word changed since the previous read (or is not 0 on the first), or whose value
    /// went down, are read again until the high word reads the same twice in a row; the low word read in between
    /// then belongs to it.
    pub fn read(&mut self, source: &mut dyn Source) -> anyhow::Result<()> {
        for block in self.blocks_mut() {
            read_block(block, source)?;
        }
        Ok(())
    }

    /// [`read`](Self::read), then what changed since the previous read: every counter as [`Event::Initial`] on the
    /// first read, afterwards the counters whose value differs.
    pub fn read_changes(&mut self, source: &mut dyn Source) -> anyhow::Result<Vec<Change<'_>>> {
        let previous: HashMap<(Storage, u64), u64> = self
            .blocks()
            .flat_map(|b| b.values().map(|(c, v)| ((b.storage(), c.idx), v.to_u64())))
            .collect();
        let first = previous.is_empty();
        self.read(source)?;

        let mut changes = vec![];
        for b in self.blocks() {
            if first {
                changes.extend(b.values().map(|(counter, value)| Change {
                    event: Event::Initial,
                    storage: b.storage(),
                    counter,
                    value,
                    previous: None,
                }));
                continue;
            }
            let changed: Vec<(&Counter, Value, u64)> = b
                .values()
                .filter_map(|(c, v)| {
                    let prev = *previous.get(&(b.storage(), c.idx))?;
                    (v.to_u64() != prev).then_some((c, v, prev))
                })
                .collect();
            let decreased = changed
                .iter()
                .filter(|(_, v, prev)| v.to_u64() < *prev)
                .count();
            // Resetting the target or the buffer zeroes all counters at once, anything else lowers single counters.
            // All counters that were non-zero going down at once is a reset, even if some have counted again since.
            let was_nonzero = previous
                .iter()
                .filter(|((storage, _), v)| *storage == b.storage() && **v != 0)
                .count();
            let is_reset = decreased > 0 && decreased == was_nonzero;
            changes.extend(changed.into_iter().map(|(counter, value, prev)| Change {
                event: if value.to_u64() > prev {
                    Event::Change
                } else if is_reset {
                    Event::Reset
                } else {
                    Event::Decrease
                },
                storage: b.storage(),
                counter,
                value,
                previous: Some(prev),
            }));
        }
        Ok(changes)
    }

    /// Set all counters of a buffer to 0.
    pub fn reset(&self, storage: Storage, source: &mut dyn Source) -> anyhow::Result<()> {
        source.reset(storage)
    }
}

fn read_block(block: &mut CountersBlock, source: &mut dyn Source) -> anyhow::Result<()> {
    let storage = block.storage();
    let mut words = vec![0u32; block.used_words() as usize];
    source.read(storage, 0, &mut words)?;
    let u64_counters: Vec<u64> = block
        .entries()
        .iter()
        .filter(|(_, c)| c.ty == Ty::U64)
        .map(|(idx, _)| *idx)
        .collect();
    for idx in u64_counters {
        let (lo, hi) = (idx as usize, idx as usize + 1);
        let previous = block.value(idx).map_or(0, |v| v.to_u64());
        // The high word read before a carry, the low word after it: the value goes down
        let lower = u64::from(words[hi]) << 32 | u64::from(words[lo]) < previous;
        if words[hi] == (previous >> 32) as u32 && !lower {
            continue;
        }
        let mut last_hi = words[hi];
        for _ in 0..TEAR_RETRIES {
            let mut pair = [0u32; 2];
            source.read(storage, idx, &mut pair)?;
            words[lo..=hi].copy_from_slice(&pair);
            if pair[1] == last_hi {
                break;
            }
            last_hi = pair[1];
        }
    }
    block.set_words(&words)
}

/// What a read in watch mode reports about a counter, the value is always the current one.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    /// First read, every counter is reported once
    Initial,
    /// Value increased since the previous read
    Change,
    /// All counters of a buffer that were non-zero decreased: the target or the counters were reset
    Reset,
    /// Value decreased while other counters of the same buffer did not. Counters saturate rather than wrap, so this
    /// is the firmware writing the buffer, or corruption
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

/// A counter reported by [`Counters::read_changes`].
#[derive(Debug)]
pub struct Change<'a> {
    pub event: Event,
    pub storage: Storage,
    pub counter: &'a Counter,
    pub value: Value,
    /// Value at the previous read, `None` for [`Event::Initial`]
    pub previous: Option<u64>,
}
