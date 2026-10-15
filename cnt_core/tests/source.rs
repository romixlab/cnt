//! Decoding words from a [`Source`] with the fixture firmware's ELF.

use cnt_core::{Counters, Event, Source, Storage};

const FIXTURE: &[u8] = include_bytes!("fixtures/counters_fw.elf");

/// Buffers in memory; `torn` makes the next read of a word return another value first.
#[derive(Default)]
struct Memory {
    ram: Vec<u32>,
    bkp: Vec<u32>,
    reads: usize,
    /// (word index, values returned by the next reads of it, in order)
    torn: Option<(usize, Vec<u32>)>,
}

impl Memory {
    fn buf(&mut self, storage: Storage) -> &mut Vec<u32> {
        match storage {
            Storage::Ram => &mut self.ram,
            Storage::Bkp => &mut self.bkp,
        }
    }
}

impl Source for Memory {
    fn read(&mut self, storage: Storage, offset: u64, words: &mut [u32]) -> anyhow::Result<()> {
        self.reads += 1;
        let offset = offset as usize;
        let len = words.len();
        words.copy_from_slice(&self.buf(storage)[offset..offset + len]);
        if storage == Storage::Ram
            && let Some((idx, values)) = &mut self.torn
            && (offset..offset + len).contains(idx)
            && !values.is_empty()
        {
            words[*idx - offset] = values.remove(0);
        }
        Ok(())
    }

    fn reset(&mut self, storage: Storage) -> anyhow::Result<()> {
        self.buf(storage).fill(0);
        Ok(())
    }
}

fn names(counters: &Counters, storage: Storage) -> Vec<(u64, String, String)> {
    let block = match storage {
        Storage::Ram => counters.ram_counters(),
        Storage::Bkp => counters.bkp_counters(),
    };
    block
        .unwrap()
        .entries()
        .values()
        .map(|c| (c.idx, c.qualified_name(), c.ty.to_string()))
        .collect()
}

#[test]
fn fixture_layout() {
    let counters = Counters::from_elf(FIXTURE).unwrap();
    let ram = [
        (0, "boots", "u32"),
        (1, "loops", "u64"),
        (3, "link/overruns", "u32"),
        (4, "uart/CrcError", "u32"),
        (5, "uart/RxBytes", "u64"),
    ];
    let bkp = [
        (0, "power_cycles", "u32"),
        (1, "radio/CrcError", "u32"),
        (2, "radio/RxBytes", "u64"),
    ];
    let owned = |l: &[(u64, &str, &str)]| -> Vec<(u64, String, String)> {
        l.iter()
            .map(|(i, n, t)| (*i, n.to_string(), t.to_string()))
            .collect()
    };
    assert_eq!(names(&counters, Storage::Ram), owned(&ram));
    assert_eq!(names(&counters, Storage::Bkp), owned(&bkp));
    assert_eq!(counters.ram_counters().unwrap().used_words(), 7);
    assert_eq!(counters.bkp_counters().unwrap().used_words(), 4);
    assert_eq!(counters.ram_counters().unwrap().capacity_words(), 16);
    assert_eq!(counters.build_id().unwrap().len(), 20);
}

fn value(counters: &Counters, storage: Storage, name: &str) -> u64 {
    counters
        .blocks()
        .filter(|b| b.storage() == storage)
        .flat_map(|b| b.values())
        .find(|(c, _)| c.qualified_name() == name)
        .map(|(_, v)| v.to_u64())
        .unwrap()
}

#[test]
fn values_and_changes() {
    let mut counters = Counters::from_elf(FIXTURE).unwrap();
    let mut mem = Memory {
        ram: vec![1, 5, 1, 0, 2, 64, 0, 0],
        bkp: vec![7, 0, 0, 0],
        ..Default::default()
    };
    let changes = counters.read_changes(&mut mem).unwrap();
    assert_eq!(changes.len(), 8);
    assert!(changes.iter().all(|c| c.event == Event::Initial));
    assert_eq!(value(&counters, Storage::Ram, "loops"), (1 << 32) + 5);
    assert_eq!(value(&counters, Storage::Ram, "uart/RxBytes"), 64);
    assert_eq!(value(&counters, Storage::Bkp, "power_cycles"), 7);

    mem.ram[3] = 1;
    let changes = counters.read_changes(&mut mem).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].counter.qualified_name(), "link/overruns");
    assert_eq!(
        (changes[0].event, changes[0].previous),
        (Event::Change, Some(0))
    );

    // A target reset zeroes the RAM buffer, the BKP one stays
    mem.reset(Storage::Ram).unwrap();
    mem.ram[4] = 1;
    let changes = counters.read_changes(&mut mem).unwrap();
    assert!(
        changes.iter().all(|c| c.event == Event::Reset),
        "{changes:?}"
    );
    assert_eq!(changes.len(), 5);

    // One counter going down while others are non-zero is not a reset
    mem.ram[0] = 1;
    mem.ram[4] = 3;
    counters.read(&mut mem).unwrap();
    mem.ram[4] = 1;
    let changes = counters.read_changes(&mut mem).unwrap();
    assert_eq!(changes[0].event, Event::Decrease);
}

#[test]
fn torn_u64_is_read_again() {
    let mut counters = Counters::from_elf(FIXTURE).unwrap();
    let mut mem = Memory {
        ram: vec![0, u32::MAX, 0, 0, 0, 0, 0],
        bkp: vec![0; 4],
        ..Default::default()
    };
    counters.read(&mut mem).unwrap();
    assert_eq!(mem.reads, 2, "nothing to re-read");

    // loops carries into its high word, the low word is read after the carry, the high word before it
    mem.ram[1..3].copy_from_slice(&[0, 1]);
    mem.torn = Some((2, vec![0]));
    mem.reads = 0;
    counters.read(&mut mem).unwrap();
    assert_eq!(value(&counters, Storage::Ram, "loops"), 1 << 32);
    // Both buffers, then the pair until its high word reads the same twice
    assert_eq!(mem.reads, 4);

    // The other way round, low word before the carry and high word after it
    mem.ram[1..3].copy_from_slice(&[0, 2]);
    mem.torn = Some((1, vec![u32::MAX]));
    mem.reads = 0;
    counters.read(&mut mem).unwrap();
    assert_eq!(value(&counters, Storage::Ram, "loops"), 2 << 32);
    assert_eq!(mem.reads, 3);
}
