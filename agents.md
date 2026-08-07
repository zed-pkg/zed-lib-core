# Agent instructions

## Scope and hierarchy

- These instructions apply to the whole `zed-pkg/zed-lib` repository unless a deeper lowercase `agents.md` adds narrower rules.
- Before editing, resolve the current working directory and load every readable ancestor `agents.md` from the filesystem root to the working directory. Do not search siblings. Resolve symlinks, deduplicate resolved files, and report unreadable or cyclic instruction files.
- `.claude/CLAUDE.md`, `.gemini/GEMINI.md`, and `.openai/AGENTS.md` are pointers only. Never duplicate instructions in tool-specific files.

## Repository role

This repository implements the contract defined by `zed-pkg/zed-interfaces`. Interfaces hold shape — types, serialization, validation; this holds behavior — resolution, planning, policy. The dependency runs one way and must stay that way: `zed-lib` depends on `zed-interfaces`, never the reverse.

## Working rules

- Do not copy a module out of `zed-interfaces`. Depend on it until it is *moved* — a duplicated implementation drifts silently, and drift between the CLI and a front end is the failure this repository exists to prevent.
- Moving behavior out of `zed-interfaces` is a breaking change for that crate. Land each move as its own change with its consumers updated in contract-first order, not as a batch.
- Cross-language behavior is specified by `conformance/cases/*.json` before it is implemented. Add the case, watch it fail, then write the code; a case written afterwards tests the code's opinion rather than the contract.
- Every implementation slice runs the same corpus. A case that only one language can satisfy is a bug in the case.
- Errors carry a stable `kind()` string shared with the corpus. Renaming one is a breaking change for every implementation and every consumer that matches on it.
- Resolution returns versions in their **published spelling**. Normalizing is for comparison only — the store address and the VCS tag must stay faithful to what the publisher tagged.
- Keep `Cargo.lock` committed; do not allow CI to update it implicitly.
- Pin GitHub Actions by immutable commit SHA and keep workflow permissions read-only unless a documented write is required.

## Validation

```sh
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The crate resolves `zed-interfaces` through a sibling checkout at
`../zed-interfaces`, so clone both into the same parent directory.
