# Formal embedding-search boundary

`embedding_search_selection.qnt`, configured by
`embedding_search_selection.fm.toml`, is the executable model for the
canonical JSONB semantic-search selector in
`src/rust-orm/registry/search.rs`.

The model checks the obligations introduced by the latest-row hardening:

1. rows are partitioned by `(entity, model)` rather than collapsed across
   models;
2. for each partition, the selected row is the greatest `updated_at`, with
   `id` as the deterministic equal-timestamp tie-breaker;
3. visibility is applied to the selected row before a result is returned; and
4. replaying the same content-row upsert does not change selection.

The finite model includes two entities, two models, duplicate content rows,
and equal-timestamp rows. It is a refinement check for the row-selection
boundary, not a claim that Quint proves PostgreSQL's planner, JSONB numeric
conversion, cosine arithmetic, or the full model-registry lifecycle in DEN-1165.
Those remain covered by the Rust validation/query tests and the planned
model-registry migration work.

## Run locally

With the schema-v1 `fmctl` runner, Node.js 22, and Java 17 or newer:

```sh
fmctl --manifest formal/embedding_search_selection.fm.toml validate
fmctl --manifest formal/embedding_search_selection.fm.toml check
fmctl --manifest formal/embedding_search_selection.fm.toml simulate
fmctl --manifest formal/embedding_search_selection.fm.toml verify
```

The manifest pins Quint `0.32.0`, bounds output to 8 MiB, and requests an
exhaustive TLC check of the finite graph. Generated evidence is written below
`.formal-artifacts/` and is intentionally ignored by Git.

## Dependency-resolution boundary

`dependency_resolution.qnt`, configured by
`dependency_resolution.fm.toml`, is the bounded safety model for the shared
Rust/TypeScript/Dart resolver. It abstracts the parser's string algebra into
finite candidate ranks and membership predicates. Selection visits candidates
one at a time; an independent quantified invariant checks the greatest eligible
candidate in the visited prefix, including the last-equal spelling rule. There
is no expected-answer table in the selection transition system.

The 17 concrete fixtures cover semver, calver, opaque exact/range requests,
normalized spelling ties in both orders, explicit prerelease ranges, malformed
requirements, empty candidate sets, unsatisfied ranges, and absent/withdrawn or
prerelease latest hints. Outcomes preserve the published spelling and the three
distinct errors, including `no_versions` precedence over malformed input.
Every fixture runs twice through the same transitions in `trace_step`.
Range selection is stable-only, even for an explicit `=1.5.0-rc.1` comparator;
exact-tag selection is verbatim. Membership and stability are separate model
predicates so this project-specific policy is not mistaken for Cargo's generic
prerelease matching behavior.

`generate_formal_corpus` projects the completed ITF states into 34 schema-validated
cases. It does not import a production resolver. Rust, TypeScript, and Dart load
those exact files and invoke their real `resolve_version` / `latest_stable`
equivalents. CI regenerates the trace and corpus and requires byte-for-byte
agreement with the committed fixtures. The existing 600-case Rust-oracle fuzz
corpus remains a separate differential test. See [generation instructions](../conformance/README.md).

This is a finite refinement check, not a proof of arbitrary semver parsing or
all possible package graphs. The three two-node graph scenarios retain DEN-99's
abstract stage/commit contract: missing dependencies and a rejected cycle clear
staged state without publishing a partial lock. They are **not** replayed against
the scalar resolver, which has no graph API. Concrete graph resolution lives in
zed-cli (including its separately defined exact-cycle policy), while registry
publication models remain in zed-api-server.rs under DEN-99. Extending graph
refinement must use those production owners and their actual cycle semantics.

Canonical ownership: this shared-resolver model and corpus belong to
[DEN-3855](https://linear.app/denman/issue/DEN-3855) and
[zed-lib-core #44](https://github.com/zed-pkg/zed-lib-core/issues/44), not a second
API-server resolver implementation. The lock waiter-lifecycle model separately
lives at `src/rust-lock/formal/`; zed-lock is incorporated into this repository.

Run it locally with:

```sh
fmctl --manifest formal/dependency_resolution.fm.toml validate
fmctl --manifest formal/dependency_resolution.fm.toml doctor
fmctl --manifest formal/dependency_resolution.fm.toml check
fmctl --manifest formal/dependency_resolution.fm.toml simulate
fmctl --manifest formal/dependency_resolution.fm.toml verify
```
