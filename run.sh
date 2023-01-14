#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"

cargo build --release

export REPO="$1"
export CONCURRENCY="$2"
export INPUT="$3"
export INPUT_BASE=$(basename "$INPUT")

parallel  -P"$CONCURRENCY" --joblog=logs/"${INPUT_BASE}".log --results=results/"${INPUT_BASE}"/ --pipe -N1 --progress ./target/release/pypi-import-test --repo="$REPO" from-stdin < "$INPUT"
