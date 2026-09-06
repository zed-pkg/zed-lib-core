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
finite published candidate identities and requirement shapes, then checks the
properties that must hold across all three implementations:

1. every modeled outcome is one of the stable success/error kinds;
2. a successful selection is present in the published candidate set;
3. opaque packages never resolve through range algebra;
4. malformed, empty, and unsatisfied requirements remain distinct; and
5. missing transitive dependencies and cycles fail closed without emitting a
   partial lock.

The model is deliberately a safety boundary, not a replacement for production
semver parsing. The schema-validated corpus remains the executable cross-language
oracle, and the larger deterministic corpus remains differential testing.

Run it locally with:

```sh
fmctl --manifest formal/dependency_resolution.fm.toml validate
fmctl --manifest formal/dependency_resolution.fm.toml check
fmctl --manifest formal/dependency_resolution.fm.toml simulate
fmctl --manifest formal/dependency_resolution.fm.toml verify
```
