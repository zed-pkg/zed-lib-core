# Source provenance

Initial standalone extraction:

- source repository: `zed-pkg/zed-cli`
- source commit: `fd3b08eb1ac170518cb795e662318ae2714b1176`
- source path: `crates/zed-lock`

Fold into the canonical core library (2026-09-04):

- folded into: `zed-pkg/zed-lib-core`
- fold path: `src/rust-lock`
- last standalone commit: `7818d0140f9947352f803d4a50aabb8e0b26265a` (zed-pkg/zed-lock main)
- the full zed-lock history is a parent of the zed-lib-core merge commit; see
  `MERGE_PROVENANCE.md` at the repository root

The extracted source had already passed Linux, macOS, Windows, strict
Clippy, cross-process contention, owner-death, and independent
`zed-pkg-test` conformance before publication.
