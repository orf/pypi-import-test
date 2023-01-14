#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"

cargo build --release

cat input_urls.txt | head -n25 | parallel -P15 --pipe -N1 --progress ./target/release/pypi-import-test --repo=/Users/tom/PycharmProjects/github/orf/pypi-code-import from-stdin
