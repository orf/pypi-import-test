#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export COMBINED_DIR="$1"
export PROCESSES="$2"
export RUST_LOG=warn

fd -a . "$COMBINED_DIR" --max-depth=1 | sort | head -n1 | parallel -u --progress --eta --xargs -N5 -P"$PROCESSES" -I@ "./target/release/pypi-import-test create-repository @ && git -C @ push origin main:main && git -C push origin import:import 2>&1"
