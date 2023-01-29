#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"

cargo build --release

export REPOS="$1"
export INPUT="$2"
export CONCURRENCY="$3"

parallel -a"$INPUT" --dry-run --progress --eta -P"$CONCURRENCY" "./target/release/pypi-import-test --repo="$REPO/{/}" from-json"
