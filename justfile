bin_dir := env_var("HOME") / ".local/bin"

# Bare `just` only lists the recipes
default:
    @just --list

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
