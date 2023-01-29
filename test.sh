#!/usr/bin/env zsh

mkdir tests/
mkdir tests/input_data/

rm -rf tests/repos/*
fd -a . tests/url_data > tests/