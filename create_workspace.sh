#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export WORKSPACE="$1"
export REPOS_DIRECTORY="$2"
export CONCURRENCY="$3"
export TEMPLATE_DIR="$4"

export LIMIT="5000"

export URLS_DIR="$WORKSPACE"/urls/
export INDEX_FILE="$WORKSPACE"/index
export PARTITIONS_DIR="$WORKSPACE"/partitions/
export TEMP_DIR="$WORKSPACE"/temp/

echo "Removing existing workspace"
#mv "$WORKSPACE" "old_$WORKSPACE" && rm -rf "old_$WORKSPACE" &

rm -rf "$WORKSPACE"
mkdir -p "$WORKSPACE"
mkdir -p "$URLS_DIR"
mkdir -p "$PARTITIONS_DIR"
mkdir -p "$TEMP_DIR"


echo "creating URLs"
./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT" --find="pulumi-azure-native.json"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT" --find="human-id.json"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --split=500 --find="$(cat tests/debug.txt)"

echo "creating index file"
fd -a . "$URLS_DIR" | shuf > "$INDEX_FILE"

echo "running partitions"
export RUST_LOG=warn
parallel -u --progress --joblog=job.log --eta -P "$CONCURRENCY" -a"$INDEX_FILE" -I{} "./target/release/pypi-import-test from-json {} $TEMP_DIR/{/} $PARTITIONS_DIR/{/} $TEMPLATE_DIR 2>&1 && echo DONE $PARTITIONS_DIR/{/}"
