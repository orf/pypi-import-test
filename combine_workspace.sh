#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export WORKSPACE="$1"
export INPUT_GIT_DIR="$2"
export CONCURRENCY="$3"
export PACKAGES_PER_PARTITION="20"

export COMBINED_DIR="$WORKSPACE"/combined
export INDEX_FILE="$WORKSPACE"/index

rm -rf "$WORKSPACE"
mkdir -p "$WORKSPACE"
mkdir -p "$COMBINED_DIR"

echo "Creating step index"
fd -a . "$INPUT_GIT_DIR" | shuf > "$INDEX_FILE"

export RUST_LOG=warn
parallel -u --progress --eta --joblog="$WORKSPACE"/job.log --xargs -n"$PACKAGES_PER_PARTITION" -P"$CONCURRENCY" -a"$INDEX_FILE" -I{} "./target/release/pypi-import-test combine {#} ${COMBINED_DIR}/{#}/ {} 2>&1"
