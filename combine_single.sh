#!/usr/bin/env zsh

export LOCATION="$1"

args=("${@:2}")

git init -q "$LOCATION"

./target/release/pypi-import-test merge-branches "$LOCATION" "${args[@]}" | git -C "$LOCATION" fast-import

git -C "$LOCATION" gc --aggressive --prune=now
