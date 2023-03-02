#!/usr/bin/env zsh

export COMBINED_DIR="$1"
export PROCESSES="$2"

fd -a . "$COMBINED_DIR" --max-depth=1 | sort | parallel -u --progress --eta --xargs -N4 -P"$PROCESSES" -I@ "git -C @ gc --aggressive --prune=now"
