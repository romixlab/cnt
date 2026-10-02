//! Counters on a host, without the `cnt.x` linker script: words are allocated on first use.
#![cfg(not(feature = "disabled"))]

use core::sync::atomic::Ordering;

#[derive(cnt::Count)]
enum Event {
    A,
    #[count(u64)]
    B,
    C,
}

static FIRST: cnt::Counters<Event> = cnt::counters!(Event, first);
static SECOND: cnt::Counters<Event> = cnt::counters!(Event, second);

fn read(buf: &[core::sync::atomic::AtomicU32]) -> Vec<u32> {
    buf.iter().map(|w| w.load(Ordering::Relaxed)).collect()
}

// Single test, the buffers are shared by everything in this binary
#[test]
fn counters_are_allocated_on_first_use() {
    for _ in 0..3 {
        cnt::cnt!(calls: u32);
    }
    cnt::cnt!(bytes: u64 += (1 << 32) + 5);
    FIRST.add(Event::B, 7);
    FIRST.count(Event::A);
    SECOND.count(Event::C);
    SECOND.count(Event::C);
    cnt::bkp_cnt!(boots: u32 += 4);

    let ram = read(cnt::counters_ram_buffer());
    assert_eq!(ram[..12], [3, 5, 1, 1, 7, 0, 0, 0, 0, 0, 2, 0]);
    assert_eq!(read(cnt::counters_bkp_buffer())[..2], [4, 0]);
}
