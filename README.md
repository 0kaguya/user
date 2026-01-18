Config file patches.

## Usage

Apply patches with `./patch.rs`. Requires Rust toolchain and some shell 
utilities.

Add githooks with `git config core.hooksPath .githooks` so the script runs
automatically on pulls and commits.

## Motivation

A workaround for those who doesn't support a built-in `config.d` approach.
