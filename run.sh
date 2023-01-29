#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"

cargo build --release

export REPOS="$1"
export INPUT="$2"
export CONCURRENCY="$3"

parallel -a"$INPUT" --progress --eta -P"$CONCURRENCY" -I@ "./target/release/pypi-import-test --repo="$REPO/{.}" from-json"
