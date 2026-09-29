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
```

Under the hood, a simple linker trick is used to obtain a unique ID for each count statement (similar to defmt).
Then an element of a `_CNT_RAM_BUFFER` is incremented. Updates compile to a plain load, add and store, there are no
critical sections or atomic read-modify-write instructions involved, so counting is safe in any context, including
interrupts, and works on cores without atomic support (e.g. Cortex-M0).

`u32` counters are supported as well, and you can pass `true` to count unconditionally:

```rust
fn process_packet() {
    cnt::cnt_if!(true, packet_count: u32);
}
```

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
  * If `--chip` is not given, it is taken from a `probe-rs run --chip <CHIP>` runner in `.cargo/config.toml`, so usually
    just `cnt read` or `cnt tui` is enough.

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
cnt_if!(true, event_count: u32 += 1 + request.len() as u32);
```

### Severity levels, default is `info`, supported: `error`, `warn`, `info`, `debug`, `trace`
```rust
cnt_if!(true, bytes_lost: u64 += 1, warn);
```

### Set group name, useful when there are many counters in use
```rust
cnt_if!(true, bytes_rx: u64 += buf.len(), debug, usart);
```

### Units
Set unit for better readability, upstream software can then convert from e.g., Bytes to KiB or MiB automatically:
```rust
cnt_if!(true, bytes_rx: u64 "B" += buf.len(), debug, usart);
```

### Non-volatile counters

`bkp_cnt_if!` takes the same arguments as `cnt_if!`, but counts into `_CNT_BKP_BUFFER`, which is meant to be placed
into memory that survives a reset, e.g. backup SRAM. To use it:

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

A counter is only ever updated from its own call site, which is what makes lock-free updates safe. If that call site
runs in several contexts, e.g. a function called from both thread mode and an interrupt, an update may be lost when the
interrupt preempts another update of the same counter. Counters are for statistics, not for exact accounting across
contexts.

## Low level

If CLI is not available, use `arm-none-eabi-nm` to view the counter indices:

```shell
arm-none-eabi-nm ./path/to/elf_fw | grep cnt_ram
```
