//! A simulated instrument on the B135 board, to show what `cnt tui` displays: a USB bulk path, a CAN bus, an ADC
//! sampler and a control loop, each a group of counters, with units, all five severities, `u32` and `u64`, an
//! instance counter group cleared by the firmware, a counter in backup registers and defmt logs.
//!
//! The story repeats every 36 s: the host starts a firmware image transfer over USB at 4 s, the CAN bus goes bus-off
//! at 17 s and recovers 2 s later (the control loop overruns meanwhile), the transfer ends at 22 s and the host resets
//! the USB bus at 26 s, which clears the `usb` counters. No peripheral besides the clock and the backup registers is
//! touched: the traffic is made up.
#![no_std]
#![no_main]

use cnt::{bkp_cnt, cnt, cnt_if};
use defmt::{error, info, unwrap, warn};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::pac;
use embassy_stm32::rcc::SupplyConfig;
use embassy_time::{Duration, Instant, Ticker};
use panic_probe as _;

/// Length of the story, it starts again after.
const CYCLE_MS: u64 = 36_000;
const USB_TRANSFER_START_MS: u64 = 4_000;
const USB_TRANSFER_END_MS: u64 = 22_000;
const USB_BUS_RESET_MS: u64 = 26_000;
const CAN_BUS_OFF_MS: u64 = 17_000;
const CAN_RECOVERED_MS: u64 = 19_000;

#[derive(cnt::Count)]
enum UsbEvent {
    #[count(u64)]
    RxPackets,
    #[count(u64, unit = "B", debug)]
    RxBytes,
    #[count(warn)]
    CrcError,
}

/// An instance group, so that the firmware can clear it on a USB bus reset.
static USB: cnt::Counters<UsbEvent> = cnt::counters!(UsbEvent, usb);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    // The board's core supply: the chip's SMPS alone (as b135_template). Clocks stay at the 64 MHz HSI.
    config.rcc.supply_config = SupplyConfig::DirectSMPS;
    let _p = embassy_stm32::init(config);
    enable_backup_registers();

    bkp_cnt!(boots: u32);
    info!("b135_demo up: USB, CAN at 500 kbit/s, ADC at 10 kS/s, control loop at 1 kHz");

    spawner.spawn(unwrap!(usb()));
    spawner.spawn(unwrap!(can()));
    spawner.spawn(unwrap!(adc()));
    spawner.spawn(unwrap!(control_loop()));
}

/// Write access to RTC_BKPxR, where the `bkp_cnt!` counters live.
fn enable_backup_registers() {
    pac::PWR.cr1().modify(|w| w.set_dbp(true));
    pac::RCC.apb4enr().modify(|w| w.set_rtcapben(true));
}

/// Milliseconds into the current story.
fn story_ms() -> u64 {
    Instant::now().as_millis() % CYCLE_MS
}

/// The USB load of the transfer: ramps up over 6 s, holds, and stops at its end. 0 to 1000 per mille.
fn transfer_load(t: u64) -> u64 {
    match t {
        t if !(USB_TRANSFER_START_MS..USB_TRANSFER_END_MS).contains(&t) => 0,
        t => ((t - USB_TRANSFER_START_MS) / 6).min(1000),
    }
}

#[embassy_executor::task]
async fn usb() {
    let mut rng = Rng(0x1234_5678);
    let mut ticker = Ticker::every(Duration::from_millis(20));
    let mut transfer_bytes = 0u64;
    let mut last = story_ms();
    loop {
        ticker.next().await;
        let t = story_ms();
        if last < USB_TRANSFER_START_MS && t >= USB_TRANSFER_START_MS {
            info!("USB: host opened the bulk endpoint, firmware image transfer started");
            transfer_bytes = 0;
        }
        if last < USB_TRANSFER_END_MS && t >= USB_TRANSFER_END_MS {
            info!("USB: transfer done, {} kB", transfer_bytes / 1000);
        }
        if last < USB_BUS_RESET_MS && t >= USB_BUS_RESET_MS {
            USB.clear();
            warn!("USB: bus reset by the host, usb counters cleared");
        }
        last = t;

        // Control transfers every 100 ms, 512 B bulk packets with the transfer
        let mut packets = (t % 100 < 20) as u64;
        let mut bytes = packets * 8;
        let bulk = transfer_load(t) * 40 / 1000;
        if bulk > 0 {
            let bulk = bulk - rng.below(bulk / 4 + 1);
            packets += bulk;
            bytes += bulk * 512;
            transfer_bytes += bulk * 512;
            if rng.below(300) == 0 {
                USB.count(UsbEvent::CrcError);
            }
        }
        USB.add(UsbEvent::RxPackets, packets);
        USB.add(UsbEvent::RxBytes, bytes);
    }
}

#[embassy_executor::task]
async fn can() {
    let mut rng = Rng(0x0bad_cafe);
    let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut last = story_ms();
    loop {
        ticker.next().await;
        let t = story_ms();
        if last < CAN_BUS_OFF_MS && t >= CAN_BUS_OFF_MS {
            cnt!(bus_off: u32, error, can);
            error!("CAN: bus-off after 248 TX errors, recovering");
        }
        if last < CAN_RECOVERED_MS && t >= CAN_RECOVERED_MS {
            info!("CAN: back on the bus after 128 x 11 recessive bits");
        }
        last = t;
        if (CAN_BUS_OFF_MS..CAN_RECOVERED_MS).contains(&t) {
            continue;
        }
        // About 500 frames/s
        let frames = 4 + rng.below(3);
        cnt!(frames: u64 += frames, info, can);
    }
}

#[embassy_executor::task]
async fn adc() {
    let mut rng = Rng(0x5eed_1234);
    let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut next_report = 12_000;
    loop {
        ticker.next().await;
        cnt!(samples: u64 += 100, debug, adc);
        // The board's load follows the USB transfer: 85 mA idle, up to 145 mA. Counted as charge, 10 ms at a time
        let load_ma = 85 + transfer_load(story_ms()) * 60 / 1000 + rng.below(5);
        cnt!(charge: u64 "mAs" += load_ma / 10, info, adc);

        let t = story_ms();
        if t >= next_report && t < next_report + 10 {
            let mv = 4960 + rng.below(30);
            info!(
                "ADC: +5V0_SYS {}.{=u64:02} V, load {} mA",
                mv / 1000,
                mv % 1000 / 10,
                load_ma
            );
            next_report = if next_report == 12_000 {
                30_000
            } else {
                12_000
            };
        }
    }
}

#[embassy_executor::task]
async fn control_loop() {
    let mut rng = Rng(0xc0ff_ee11);
    let mut ticker = Ticker::every(Duration::from_millis(1));
    loop {
        ticker.next().await;
        cnt!(ticks: u64, info, ctrl);
        // The loop's work, 180-260 us; the CAN recovery steals the core now and then
        let mut busy = 180 + rng.below(80);
        let t = story_ms();
        if (CAN_BUS_OFF_MS..CAN_RECOVERED_MS).contains(&t) && rng.below(150) == 0 {
            busy += 900;
        }
        cnt!(busy: u64 "µs" += busy, trace, ctrl);
        cnt_if!(busy > 1000, overruns: u32, warn, ctrl);
    }
}

/// xorshift32: made-up traffic needs no better randomness.
struct Rng(u32);

impl Rng {
    /// A number in `0..n`, `n` > 0.
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as u64 % n
    }
}
