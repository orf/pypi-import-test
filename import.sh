#!/usr/bin/env bash

args=$#
for (( i=2; i<=$args; i++ ))
do
    cd ${!i};
    git tag -f import_$1_$i;
    git fast-export refs/tags/import_$1_$i;
done
