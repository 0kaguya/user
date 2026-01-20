# Instructions

## Developing `patch.rs`

- Run cargo commands on `patch.rs` with `./dev.sh ...`.

## Description on config files' paths

- Configs in `patches` will be merged into config under user's $HOME, in the
  same directory hierarchy, where `dot-` will be replaced by `.` and 
  `<filename>.d` maps to `<filename>` itself.

- Files like `AGENTS.md` and `README.md` will not be merged.

## Writing configs

- Check updates on requirements with `git diff` on README.md.
