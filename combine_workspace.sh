#!/usr/bin/env zsh

export RUSTFLAGS="-Ctarget-cpu=native"
cargo build --release

export COMBINED_DIR="$1"
export TARGET_DIR="$2"
export PROCESSES="$3"
export RUST_LOG=warn

export RAYON_NUM_THREADS="25"
fd -a . "$TARGET_DIR" | head -n10 | sort | parallel -u --progress --eta --xargs -N10 -P"$PROCESSES" -I@ "./combine_single.sh $COMBINED_DIR/{#} @"

#
#export COMBINED_DIR="$1"
#export WORKSPACE="$2"
#export BASE_REPO="$3"
#export INDEX_FILE="$COMBINED_DIR"/index
#export REPOS_PER_SPLIT=20000
#
#mkdir "$COMBINED_DIR"
#mkdir "$COMBINED_DIR/splits/"
#mkdir "$COMBINED_DIR/repos/"
#
#echo "Creating step index"
#fd -a . "$WORKSPACE/partitions" --maxdepth=1 -t=d | rg -v '(/mastapy_0|/delphixpy_0|/pulumi-azure-native_0|/ansible_0|/dbpedia-ent_0|/intersight_0|/itk-filtering_0|/easyvisualize_0|/lusid-sdk-preview_7|/msgraph-beta-sdk_0|/ixnetwork-restpy_0)\.json$' | shuf > "$INDEX_FILE"
#
#split -l "$REPOS_PER_SPLIT" "$INDEX_FILE" "$COMBINED_DIR/splits/a"
#
#fd . "$COMBINED_DIR/splits/" | parallel --progress --eta -u -P6 -I@ "./combine_single.sh $COMBINED_DIR/repos/{#}/ $BASE_REPO @"
