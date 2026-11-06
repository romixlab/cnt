# b135_demo

A demo firmware for the B135 board (STM32H725IG) that shows what `cnt tui` displays. Nothing but the clock and the
RTC backup registers is touched: the traffic of a USB path, a CAN bus, an ADC and a control loop is made up, and tells
a story that repeats every 36 s (see `src/main.rs`).

Run it with a probe on the board's SWD: `just demo` in the repository, or `cargo run --release` here, flashes it and
opens the TUI (the runner is `cnt run --tui`). `cnt tui --chip STM32H725IGKx` here attaches to a board already
running it. `cnt reset --bkp` zeroes the boot counter once, as the backup registers keep what was in them before.
