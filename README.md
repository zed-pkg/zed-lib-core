# zed-lib-core

Canonical core library for the **zed-pkg** registry. This repository is the
semantic merge of two predecessors — `zed-pkg/zed-lib` (resolution, polyglot
behavior, and registry entities) and `zed-pkg/zed-orm-core` (the opaque
role-aware query boundary) — with both histories preserved.

`zed-lib-core` is itself a zed package: see `.zpkg.toml`. Consumer migration and
exact source commits are recorded in [`PREDECESSOR_MIGRATION.md`](PREDECESSOR_MIGRATION.md).

## Layout

| Path | Package | What it is |
| --- | --- | --- |
| `src/rust-orm` | `zed-orm-core` | SeaORM registry entities, named data-plane operations, migration runner, and opaque connection boundary |
| `src/rust` | `zed-lib` | Version resolution and policy over the shared contract types |
| `src/ts` | `@zed-pkg/zed-lib` | The same resolution behavior, natively in TypeScript |
| `src/dart` | `zed_lib` | The same resolution behavior, natively in Dart |
| `conformance` | `zed-lib-conformance` | The shared corpus all three are held to |

The language package names remain compatible. Their repository metadata points
to `zed-pkg/zed-lib-core`; the predecessor repositories are no longer release
authorities.

## The data plane (`src/rust-orm`)

Three rules define the crate:

1. **The schema is not ours.** Every table is defined in
   `pg-defs/schema/orgs/zed-pkg/registry.sql` in
   [`k8s-libs-and-shared-defs`](https://github.com/ORESoftware/k8s-libs-and-shared-defs)
   at the revision pinned in `shared-defs.lock.json`. That file is vendored to
   `src/rust-orm/sql/registry.sql` and applied verbatim; this crate authors no
   DDL of its own.
2. **Raw sessions do not escape.** Consumers get an opaque `ReadContext` or
   `WriteContext` and call named operations in `read`, `registry`, `write`, and
   the feature-gated `invitations` module. SeaORM connections and query builders
   stay private to the crate.
3. **Writes are opt-in.** Default builds cannot compile a write symbol
   (`compile_fail` doctests prove it). API servers enable `read-write`; only the
   discrete DPM migration job enables `migrate`. The feature split expresses
   intent — the authoritative control is the database principal, because Cargo
   features are additive across a dependency graph.

### Named operation groups

- `read`: users, organizations, projects, packages, versions, licenses, and
  package search through the default read-only surface.
- `write`: identity projection, org/project/package creation, visibility
  transition, compatibility download recording, and invitation creation.
- `registry`: cross-entity text/semantic search plus read-write-gated upload,
  full download evidence, package licenses, embedding upserts, and immutable
  dependency-graph documents with normalized reverse-impact edges.
- `invitations`: atomic one-time organization/project invitation acceptance,
  compiled only for API write builds.

The API tier owns authentication and authorization. These operations own input
validation, schema relationships, transaction boundaries, source redaction,
and fail-closed persistence behavior.

### Dependency graphs

`zed_dependency_graph_artifacts.document` is the lossless JSON authority for a
declared or resolved graph. Its `sha256:` semantic digest is immutable. The
ordered `zed_dependency_graph_edges` rows are a relational index derived from
that document for neighborhood and reverse-impact queries; they are never an
independent serialization authority.

`registry::persist_dependency_graph` validates and commits the document and
all edges in one transaction. An exact retry returns the original artifact id,
while a digest or declared-root replay with different facts fails closed.
Read-only consumers use visibility-scoped named operations: fetch by digest,
fetch the newest graph for a root package version, or query incoming edges for
a registry coordinate. Private consumer graphs are filtered by the root
package's organization rather than leaked through a public dependency target.

### Where the tables live

`public`, behind a `zed_` prefix — not a dedicated schema. The pg-defs contract
tooling keys tables by **bare** name (`sql-contract.mjs` rejects duplicates;
`generate.mjs` derives every generated identifier from it), so a `zed` schema
would produce `orgs`/`users`/`projects` that collide with the `fiducia` schema
across the generated adapters. Use `schema::qualified()` rather than
interpolating table names.

### Identity

Supabase Auth is the identity provider. `shared-auth-server.rs` verifies the
Supabase JWT, owns the principal, and issues the session cookie — customer
principals on the `customer-auth` RDS instance, operator/admin principals on
`admin-auth`. **No session state lives in the registry.**

A principal maps to exactly one registry user through
`zed_users.shared_auth_subject` + `zed_users.auth_realm`. Those instances are
separate databases, so there is deliberately no foreign key;
`write::upsert_user_from_session` is what keeps the two planes consistent.

### The private → public promotion rule

A private package may be made public only while it is at most **10 days old**
and has at most **50 recorded downloads**.

Enforcement is in the database: `zed_packages_visibility_guard` re-evaluates the
rule inside the UPDATE and raises the dedicated SQLSTATEs `ZD001` (too old) and
`ZD002` (too many downloads). `write::set_package_visibility` pre-checks the
same limits so a user gets a clear message instead of a raw exception, and
`OrmError::from_db_err` maps those SQLSTATEs to typed variants so a promotion
that races past the pre-check still surfaces as the same client error.

The limits are read from `zed_public_conversion_max_age_days()` and
`zed_public_conversion_max_downloads()` rather than hardcoded, so the policy
changes in one place. Both layers use `>` rather than `>=`: a package sitting
exactly on a boundary still promotes.

### Search vectors

The canonical contract deliberately does not require the PostgreSQL `vector`
extension. Embeddings are JSONB arrays with exact dimensions and content
SHA-256 evidence. `registry::semantic_search` computes visibility-aware cosine
scores from those arrays, while a future runtime-owned ANN index may accelerate
the same contract without changing stored data or package interfaces.

## Development

```sh
cargo test  -p zed-orm-core --all-features   # entities, policy, public surface
cargo test  -p zed-orm-core                  # default read-only surface
cargo clippy -p zed-orm-core --all-features -- -D warnings
```

The live database probes (`ORM_CORE_TEST_DATABASE_URL`) are `#[ignore]` by
default and must point at a disposable database — one of them attempts DDL to
prove the read-only identity is denied.
