# Agent instructions

## Parent / root agent contract

The fleet-wide parent lives at:

- GitHub: https://github.com/oresoftware/my-ai/AGENTS.md
- Canonical disk path: `~/codes/oresoftware/my-ai/AGENTS.md`
- `~/codes/AGENTS.md` is a symlink to `~/codes/oresoftware/my-ai/AGENTS.md` (installed by `~/codes/oresoftware/my-ai/setup-final.sh`)

When this file and the parent disagree: follow this file for this repository's local layout and tools; follow the parent for org-wide conventions and the functional programming rules.

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
- The slices are native implementations, not bindings, and they take no semver dependency. `pub_semver` and npm's `semver` implement a different dialect from Cargo's — a bare `1.0.0` is exact there and a caret range here — so a wrapper would smuggle in disagreements the corpus was built to prevent. When behavior is ambiguous, the Rust `semver` crate is the reference: probe it, then pin the answer as a case.
- A new corpus case must be verified against Rust *first*. Rust is the slice consumers already run through `zed-cli`, so a case Rust fails is a case that mis-states the contract.
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

## Code style and coding patterns

remember to modularize the rust, typescript and dart - not everything belongs in main.rs, main.ts and main.dart; also follow functional coding principles - fewer side-effects (use pure functions more), more immutability (immutable variables); but for stateful apps like the client or stateful servers like websockets or tcp connections, sometimes classes and oop make more sense than functional programming perse, but we can still adhere to functional programming more than usual. Favor exhaustive pattern matching and use formal methods checking too. Favor composability and re-use , so basically create more utility functions and routines for shared use. You can follow a medium level of D.R.Y. (don't repeat yourself) - in other words you can repeat yourself at medium amount (not too much not too little). Some chaining is totally fine, so either method-chaining (immutable sometimes although with classes can be mutable too for performance), and chaining via the pipe operator is ok in languages like gleamlang.

Functional programming is mostly the following:

+ explicit inputs
+ explicit outputs
+ immutable values
+ pure transformations
+ typed errors
+ explicit state transitions
+ composition
+ effects pushed outward
+ illegal states excluded by types

## Functional programming conformance

This repository carries an FP conformance ratchet. Before you land a change:

```sh
python3 tools/fp-conformance/fp_conformance.py .
```

CI compares your findings against `tools/fp-conformance/budget.json` and fails
only when a rule's count *increases*. Do not raise the budget to get green — fix
the new violations. When you clear a class of violation, lower the budget in the
same commit with `--write-budget`.

The principles, the rule codes and the remedy for each are in `FP-GUIDELINES.md`.
