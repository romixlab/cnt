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
Then an element of a `_CNT_RAM_BUFFER` is increment.

`u32` counters are supported as well, and you can pass `true` to count unconditionally:

```rust
fn process_packet() {
    cnt::cnt_if!(true, packet_count: u32);
}
```

## How to use

* Add `cnt = "0.2"` to `Cargo.coml`
* Add `"-C", "link-arg=-Tcnt.x",` to `config.toml`
* Optionally set `CNT_RAM_BUFFER_SIZE_WORDS` in the `[env]` section as well, default value is 64 words (256 bytes).
* Flash your firmware and run the CLI tool:
  * To read once: `cnt_cli <PATH_TO_ELF> read`
  * Tu run TUI: `cnt_cli <PATH_TO_ELF> tui`
  
## How to get counters data from fw itself

Call `counters_ram_buffer`:

```rust
fn main() {
    let counters: &[u32] = cnt::counters_ram_buffer();
    // send using whatever interface to host
}
```

## Advanced usage

### Any expression can be used instead of 1
```rust
cnt_if!(true, event_count: u32 += 1 + request.len());
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

## Low level

If CLI is not available, use `arm-none-eabi-nm` to view the counter indices:

```shell
arm-none-eabi-nm ./path/to/elf_fw | grep cnt_ram
```
