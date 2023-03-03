#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export COMBINED_DIR="$1"
export PROCESSES="$2"
export RUST_LOG=warn

fd --max-depth=1 -t=d . "$COMBINED_DIR" -X "printf" '%s\n' '{/}' | sort -h | head -n30 | parallel -u --progress --eta --xargs -N1 -P"$PROCESSES" -I@ "./target/release/pypi-import-test create-repository $COMBINED_DIR/@ && git -C @ push --force origin main:main && git -C @ push --force origin import:import 2>&1"