//! What the TUI shows for the fixture firmware's counters.

use cnt_core::{Counters, Source, Storage};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const FIXTURE: &[u8] = include_bytes!("../../cnt_core/tests/fixtures/counters_fw.elf");

struct Memory([Vec<u32>; 2]);

impl Source for Memory {
    fn read(&mut self, storage: Storage, offset: u64, words: &mut [u32]) -> anyhow::Result<()> {
        let buf = &self.0[storage as usize];
        let offset = offset as usize;
        words.copy_from_slice(&buf[offset..offset + words.len()]);
        Ok(())
    }

    fn reset(&mut self, storage: Storage) -> anyhow::Result<()> {
        self.0[storage as usize].fill(0);
        Ok(())
    }
}

#[test]
fn shows_names_and_values() {
    let mut counters = Counters::from_elf(FIXTURE).unwrap();
    let mut source = Memory([vec![3, 5, 1, 0, 2, 0, 0], vec![0, 0, 100, 0]]);
    let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
    cnt_view::tui::draw_once(&mut terminal, &mut counters, &mut source, "test").unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .chunks(110)
        .map(|line| line.concat() + "\n")
        .collect();
    for text in [
        "Counters · test · 4 of 8 non-zero",
        "uart/CrcError",
        "4294967301",
        "radio/RxBytes",
        "100B",
    ] {
        assert!(screen.contains(text), "{text:?} not in\n{screen}");
    }
}
