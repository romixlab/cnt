# cnt

![Crates.io Version](https://img.shields.io/crates/v/cnt)

<img align="right" src="https://github.com/romixlab/cnt/blob/main/assets/logo.png?raw=true" alt="logo"/>

> When logging is not an option - count

In microcontroller firmwares it is not always possible or desirable to log things, for example due to:
* Timing constraints in interrupts
* Memory constraints
* Absence of a logging interface
* Absence of log recording from firmware boot, making accurate calculations impossible
* Inconvenience of analyzing log output

This crate provide a convenient way to count events, errors or anything else, using a RAM array:

```rust
fn high_frequency_irq() {
    let r = do_something();
    cnt::cnt_if!(r.is_err(), unpack_errors: u64);
}

fn process_packet() {
    cnt::cnt!(packet_count: u32);
}
```

Under the hood, a simple linker trick is used to obtain a unique ID for each count statement (similar to defmt).
Then an element of a `_CNT_RAM_BUFFER` is incremented. There are no critical sections involved, so counting is safe in
any context, including interrupts. On cores with atomic read-modify-write instructions (Cortex-M3 and up) counters are
updated with `ldrex`/`strex`, so a counter shared between thread mode and an interrupt never loses an update; on cores
without them (Cortex-M0/M0+) a plain load, add and store is used.

Library crates can count per instance, see [Instance counters](#instance-counters).

## How to use

* Add `cnt = "0.3"` to `Cargo.toml` (the dependency must not be renamed, generated code refers to `cnt`)
* Add `"-C", "link-arg=-Tcnt.x",` to `.cargo/config.toml`
* Optionally set `CNT_RAM_BUFFER_SIZE_WORDS` in the `[env]` section as well, default value is 64 words (256 bytes).
  Linking fails with a `cnt: too many RAM counters` error if the buffer is too small for the counters in use.
* Flash your firmware and run the CLI tool:
  * To read once: `cnt read --chip <CHIP> <PATH_TO_ELF>`
  * To run TUI: `cnt tui --chip <CHIP> <PATH_TO_ELF>`
  * `<PATH_TO_ELF>` can be omitted when running inside the firmware project, the most recently built binary is used.
  * Probe and target are selected the same way as in probe-rs: `--chip`, `--probe VID:PID[:SERIAL]`, `--protocol`,
    `--speed`, `--connect-under-reset`, `--chip-description-path`, or the matching `PROBE_RS_*` environment variables.
    If multiple probes are connected and `--probe` is not given, you will be asked to pick one.
  * If `--chip` is not given, it is taken from a `probe-rs run --chip <CHIP>` or `cnt run --chip <CHIP>` runner in
    `.cargo/config.toml`, so usually just `cnt read` or `cnt tui` is enough.
  * For scripts, CI and agents: `--format json` prints one JSON document, `--format jsonl` one JSON object per counter
    and line (also `CNT_FORMAT=json`), e.g. `cnt read --format jsonl | jq 'select(.value > 0)'`. Applies to `list`,
    `read` and `reset`; JSON goes to stdout, diagnostics to stderr.
  * To follow changes: `cnt read --watch [--interval <MS>]` prints all counters, then every counter whose value changed,
    with its current value. With `--format jsonl` each line has an `event` (`initial`, `change`, `reset` when all
    non-zero counters of a buffer went down, `decrease` when only some did) and a `ts` in Unix milliseconds, e.g.
    `cnt read --watch --format jsonl | jq 'select(.event == "reset")'`.
  * defmt logs of the firmware (defmt-rtt) are shown together with the counters in `cnt read --watch` and `cnt tui`,
    using the same probe connection, `--no-defmt` turns them off. In `jsonl` they are `{"event": "log", ...}` lines.
  * `cnt run` flashes the firmware, resets the target and then works like `cnt read --watch` (or `cnt tui` with
    `--tui`), so it can be the cargo runner instead of `probe-rs run`:
    ```toml
    [target.'cfg(all(target_arch = "arm", target_os = "none"))']
    runner = "cnt run --chip STM32H533RE"
    ```

<img src="https://github.com/romixlab/cnt/blob/main/assets/tui.gif?raw=true" alt="TUI demo">
  
## How to get counters data from fw itself

Call `counters_ram_buffer`, it returns the buffer as `&[AtomicU32]`, as the values keep changing while it is read:

```rust
use core::sync::atomic::Ordering;

fn main() {
    for word in cnt::counters_ram_buffer() {
        let value = word.load(Ordering::Relaxed);
        // send using whatever interface to host
    }
}
```

`u64` counters occupy two words, low word first, and might be torn if read while being incremented.

## Advanced usage

### Any expression can be used instead of 1
```rust
cnt!(event_count: u32 += 1 + request.len() as u32);
```

### Severity levels, default is `info`, supported: `error`, `warn`, `info`, `debug`, `trace`
```rust
cnt!(bytes_lost: u64, warn);
```

### Set group name, useful when there are many counters in use
```rust
cnt!(bytes_rx: u64 += buf.len() as u64, debug, usart);
```

### Units
Set unit for better readability, upstream software can then convert from e.g., Bytes to KiB or MiB automatically:
```rust
cnt!(bytes_rx: u64 "B" += buf.len() as u64, debug, usart);
```

### Instance counters

A `cnt!` counter belongs to its call site: if a driver crate is used twice in a firmware, both instances count into the
same counter. For per-instance counters the driver declares its events as an enum and takes a `&'static Counters<E>`,
and the firmware decides how many instances there are, what they are called and where they live:

```rust
// In the driver crate
#[derive(cnt::Count)]
pub enum FramerEvent {
    CrcError,                               // u32, info
    #[count(warn)]
    Overrun,
    #[count(u64, unit = "B", debug)]
    RxBytes,
}

pub struct Framer { cnt: &'static cnt::Counters<FramerEvent>, /* ... */ }

impl Framer {
    pub fn feed(&mut self, byte: u8) {
        self.cnt.add(FramerEvent::RxBytes, 1);
        self.cnt.count_if(crc_failed, FramerEvent::CrcError);
    }
}

// In the firmware
static UART1_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::counters!(FramerEvent, uart1);
static RADIO_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::bkp_counters!(FramerEvent, radio);

let uart1 = Framer::new(&UART1_FRAMER_CNT);
let radio = Framer::new(&RADIO_FRAMER_CNT);
```

Host tools show them as `uart1/CrcError`, `radio/CrcError` and so on, with the location of the `counters!` line. The
counters live in the same `_CNT_RAM_BUFFER`/`_CNT_BKP_BUFFER` as `cnt!` counters, so `counters_ram_buffer()` still
returns everything. Increments compile to the same code as `cnt!`, the variant is a constant offset from the instance.

### Disabling counters

The `disabled` feature of `cnt` compiles all counters out: the macros expand to nothing (arguments are still
type-checked, but not evaluated), `Counters<E>` becomes a zero-sized no-op and no buffers exist. A library crate can
depend on `cnt` unconditionally and let its users opt in, or a firmware can measure the overhead of counting:

```toml
[dependencies]
cnt = { version = "0.4", features = ["disabled"] }
```

`cnt.x` is still generated (empty), so a `-Tcnt.x` link argument keeps working. `cnt::DISABLED` tells at compile time
whether counters are active.

### Non-volatile counters

`bkp_cnt!` and `bkp_cnt_if!` take the same arguments as `cnt!` and `cnt_if!`, but count into `_CNT_BKP_BUFFER`, which
is meant to be placed into memory that survives a reset, e.g. backup SRAM. To use it:

* Set `CNT_BKP_BUFFER_SIZE_WORDS` in the `[env]` section of `.cargo/config.toml`, the default is 0.
* Add a `BKPSRAM` region to `memory.x`, or set `CNT_BKP_MEMORY_REGION` to the name of an existing region:
  ```ld
  MEMORY
  {
    /* ... */
    BKPSRAM : ORIGIN = 0x40024000, LENGTH = 4K
  }
  ```
* Enable the backup domain in the firmware before counting (clock, write access, battery or VBAT), as it is chip
  specific and not done by this crate.

The buffer is never initialized by the startup code, so on the first boot it contains whatever the memory did; zero it
once yourself or with `cnt reset --bkp`. In the TUI, `R` resets BKP counters, `r` RAM counters.

### Limitations

On cores without atomic read-modify-write instructions (`target_has_atomic = "32"` not set, e.g. Cortex-M0/M0+) a
counter updated both from thread mode and from an interrupt may lose an update when the interrupt preempts another
update of the same counter. On all cores a `u64` counter is two words, so reading it while it is being incremented may
return a torn value. Counters are for statistics, not for exact accounting.

Instance counters must live in a `static`, as their space is allocated by the linker; dynamically created instances
have to share one.

## Low level

If CLI is not available, use `arm-none-eabi-nm` to view the counter indices:

```shell
arm-none-eabi-nm ./path/to/elf_fw | grep '"kind":"counter"'
```
