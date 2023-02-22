#!/usr/bin/env zsh

export REPO="$1"
export BASE_REPO="$2"
export PARTITION="$3"

echo "Creating repo"
cp -r "$BASE_REPO" "$REPO/"

echo "Copying partition file $PARTITION into $REPO"

git init -q "$REPO"

cat "$PARTITION" | parallel -P300 -I@ "cp -f @/.git/objects/pack/* $REPO/.git/objects/pack/ && git -C @ show-ref --heads" | rg -v "heads/main$" > "$REPO"/.git/packed-refs
echo "Copied, repacking"
./target/release/pypi-import-test merge-branches "$REPO" "merge_$PARTITION"
git -C "$REPO" repack --max-pack-size=1500m -k -a -d -f --threads=8
