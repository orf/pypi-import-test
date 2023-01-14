#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"

cargo build --release

export REPO="$1"
export CONCURRENCY="$2"

parallel -a input_urls.txt -P"$CONCURRENCY" --joblog=job.log --results=results/ --pipe -N1 --progress ./target/release/pypi-import-test --repo="$REPO" from-stdin
