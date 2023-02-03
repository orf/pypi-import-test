#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release -F warn_log

export WORKSPACE="$1"
export INPUT_GIT_DIR="$2"
export CONCURRENCY="$3"
export PACKAGES_PER_PARTITION="10"

export COMBINED_DIR="$WORKSPACE"/combined
export INDEX_FILE="$WORKSPACE"/index

rm -rf "$WORKSPACE"
mkdir -p "$COMBINED_DIR"
mkdir -p "$WORKSPACE"

echo "Creating step index"
fd -a . "$INPUT_GIT_DIR" | shuf > "$INDEX_FILE"

export RUST_LOG=warn
parallel --progress --eta --joblog=combine.log --results=combine_results/ --xargs -n"$PACKAGES_PER_PARTITION" -P"$CONCURRENCY" -a"$INDEX_FILE" -I{} "./target/release/pypi-import-test combine ${COMBINED_DIR}/{#}/ {} 2>&1"
