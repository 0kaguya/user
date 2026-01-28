Config file management.

## Config house

Apply stowfiles with `stow stowfiles`.

## Patching

Apply patches with `./patch.rs`. Requires Rust toolchain and some shell 
utilities.

### Motivation

The `patch.rs` script is here as a workaround for those who doesn't have a
built-in `config.d` approach.

## Automatic update

Add githooks with `git config core.hooksPath .githooks`. It runs both `stow`
and `patch.rs` on commit and pull.

Add git filter for fish format by applying patches and adding `*.fish filter=fish_indent`
into `.git/info/attributes`.
