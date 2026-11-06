bin_dir := env_var("HOME") / ".local/bin"

# Bare `just` only lists the recipes
default:
    @just --list

# Tests of the workspace; `--changed`: only the crates changed against main and those that depend on them
test *flags:
    if [ "{{flags}}" = "--changed" ]; then tpm test changed -- cargo nextest run --workspace; else {{just_executable()}} test-all {{flags}}; fi

# All tests of the workspace, doctests included
test-all *flags:
    cargo nextest run --workspace {{flags}}
    cargo test --workspace --doc --quiet

# Format check and clippy without warnings: the workspace and the demo firmware
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cd examples/b135_demo && cargo fmt --check && cargo clippy --release -j 4 -- -D warnings

# Flash the demo firmware onto a B135 board through the probe and show its counters in the TUI
demo:
    cd examples/b135_demo && cargo run --release -j 4

# Build cnt CLI in release mode and install it into ~/.local/bin
install-local:
    #!/usr/bin/env bash
    set -euo pipefail
    if cargo install --list | grep -q '^cnt_cli '; then
        echo "note: removing cargo-installed cnt_cli (~/.cargo/bin/cnt) to avoid PATH shadowing"
        cargo uninstall cnt_cli
    fi
    cargo build --release -p cnt_cli
    mkdir -p "{{bin_dir}}"
    install -m 755 target/release/cnt "{{bin_dir}}/cnt"
    echo "installed $("{{bin_dir}}/cnt" --version 2>/dev/null || echo cnt) -> {{bin_dir}}/cnt"

# Rebuild the test firmware whose ELF the cnt_core tests decode
fixtures:
    cd fixtures/counters_fw && cargo build --release
    cp fixtures/counters_fw/target/thumbv7em-none-eabihf/release/counters_fw cnt_core/tests/fixtures/counters_fw.elf
