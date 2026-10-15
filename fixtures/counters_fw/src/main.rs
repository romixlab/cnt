//! Counters of every kind the host decodes: `cnt!` call sites in both buffers, u32 and u64, units, severities, and
//! `counters!` instances of a `Count` enum. Never runs: host tests serve words of their choice in its layout.
#![no_std]
#![no_main]

use panic_halt as _;

#[derive(cnt::Count)]
enum LinkEvent {
    #[count(warn)]
    CrcError,
    #[count(u64, unit = "B")]
    RxBytes,
}

static UART: cnt::Counters<LinkEvent> = cnt::counters!(LinkEvent, uart);
static RADIO: cnt::Counters<LinkEvent> = cnt::bkp_counters!(LinkEvent, radio);

#[cortex_m_rt::entry]
fn main() -> ! {
    cnt::cnt!(boots: u32);
    cnt::bkp_cnt!(power_cycles: u32);
    loop {
        cnt::cnt!(loops: u64);
        cnt::cnt!(overruns: u32, error, link);
        UART.count(LinkEvent::CrcError);
        UART.add(LinkEvent::RxBytes, 64);
        RADIO.count(LinkEvent::CrcError);
        RADIO.add(LinkEvent::RxBytes, 32);
    }
}
