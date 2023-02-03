#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export WORKSPACE="$1"
export REPOS_DIRECTORY="$2"
export CONCURRENCY="$3"

export PACKAGES_PER_PARTITION="1000"
export LIMIT="10"

export SPLITS_DIR="$WORKSPACE"/splits/
export URLS_DIR="$WORKSPACE"/urls/
export INDEX_FILE="$WORKSPACE"/index
export SPLITS_INDEX_FILE="$WORKSPACE"/splits-index
export PARTITIONS_DIR="$WORKSPACE"/partitions/

echo "Removing existing workspace"
#mv "$WORKSPACE" "old_$WORKSPACE" && rm -rf "old_$WORKSPACE" &
rm -rf "$WORKSPACE"

mkdir -p "$WORKSPACE"
mkdir -p "$SPLITS_DIR"
mkdir -p "$URLS_DIR"
mkdir -p "$PARTITIONS_DIR"

echo "creating URLs"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR"
./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT" --find="pulumi-azure-native.json"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT" --find="human-id.json"
#./target/release/pypi-import-test create-urls "$REPOS_DIRECTORY" "$URLS_DIR" --limit="$LIMIT" --find="django.json"

echo "creating index file"
fd -a . "$URLS_DIR" | shuf > "$INDEX_FILE"

#echo "splitting files into partitions"
#split -d -l "$PACKAGES_PER_PARTITION" "$INDEX_FILE" "$SPLITS_DIR"

#echo "indexing splits"
#fd --base-directory="$SPLITS_DIR" . > "$SPLITS_INDEX_FILE"

#echo "creating partitions directory"
#parallel -a "$SPLITS_INDEX_FILE" -I@ 'mkdir -p $PARTITIONS_DIR/@'

echo "running partitions"
#parallel --progress --eta -P1 -a "$SPLITS_INDEX_FILE" -I@ './run_partition.sh $SPLITS_DIR/@ $PARTITIONS_DIR/@ && echo DONE @'
export RUST_LOG=info
parallel -u --progress --eta -P "$CONCURRENCY" -a"$INDEX_FILE" -I{} "./target/release/pypi-import-test from-json {} $PARTITIONS_DIR/{/} && echo DONE $PARTITIONS_DIR/{/}"
