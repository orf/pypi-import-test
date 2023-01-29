#!/usr/bin/env zsh

#export RUSTFLAGS="-Ctarget-cpu=native"
#cargo build --release

export PARTITION_FILE="$1"
export PARTITION_DIRECTORY="$2"

parallel --progress --eta -a"$PARTITION_FILE" -I{} "./target/release/pypi-import-test from-json {} $PARTITION_DIRECTORY/{/}"

#parallel -a"$INPUT" --progress --eta -P"$CONCURRENCY" -I{} "./target/release/pypi-import-test --repo="$REPOS/{/}" from-json {}"
