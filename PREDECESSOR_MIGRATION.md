# `zed-lib` and core-lineage migration into `zed-lib-core`

`zed-pkg/zed-lib-core` is the only repository-level release authority for the
shared Zed package behavior, polyglot libraries, conformance corpus, and opaque
registry ORM.

The core repository referred to colloquially as `zed-core` is the connected
GitHub repository `zed-pkg/zed-orm-core`. Its Rust crate name remains
`zed-orm-core` for source compatibility, but its Git source and release authority
are now this repository under `src/rust-orm`.

## Preserved history

The two-history merge commit is:

```text
f27f72cc65640407409d38953c8d30ee4c95f3a6
```

Its parents are:

```text
430aafe24b6c3ab1263f1351ab4941545f592f19  zed-lib lineage
a5dabf3685db94ffdf5ae30cb3b3e4cc1cce298f  zed-orm-core lineage
```

The semantic fold is:

```text
9fdc5fed96b707b99b3b02e6541060831c3d70fd
```

Certification and package/API/storage contracts merged through
`zed-lib-core#1` as `171ee6a3ba82a492409ef86e27af793574942447`.

## Repository paths

| Predecessor responsibility | Canonical path |
| --- | --- |
| Rust version and package behavior | `src/rust` |
| Dart behavior | `src/dart` |
| TypeScript behavior | `src/ts` |
| Shared language-neutral vectors | `conformance` |
| Opaque SeaORM crate `zed-orm-core` | `src/rust-orm` |
| Canonical registry SQL evidence | `src/rust-orm/sql/registry.sql` |
| Historical shared-definitions import provenance | `shared-defs.lock.json` |
| API/page/R2 contracts | `contracts` |

## `zed-lib#7`: one-time invitation acceptance

Predecessor head:

```text
92fcbdec93992ec279b2065a1cb8ccc2fca63c9b
```

Canonical port:

```text
zed-lib-core#2
79c30f65c676f6eb304effe2a7abf969f22f2da8
```

Every behavior is retained: bounded URL-safe tokens, SHA-256 lookup, verified
email matching, one generic non-enumerating failure, org/project targets,
conditional one-winner consumption, atomic membership creation, and no role
downgrade. The canonical implementation additionally uses opaque `WriteContext`,
realm-scoped UUID identities, `zed_*` tables, revocation filtering, and
`accepted_by_user_id` evidence.

## `zed-lib#5`: registry data plane

Predecessor head:

```text
f6f240a72eb100b858ae74f2c633d09528b61805
```

The canonical implementation merged through
[`zed-lib-core#3`](https://github.com/zed-pkg/zed-lib-core/pull/3) as:

```text
d9a1f72baad87a0bbe256ad892d61d7a4fdd9135
```

It passed Rust, TypeScript, Dart, default/read-only ORM, explicit `read-write`
ORM, Clippy, conformance-corpus, source-history, schema, API/R2, and provenance
checks on its exact head. The old branch was based on a transitional schema and
raw SeaORM sessions. Its substantive requirements map as follows.

| Predecessor item | Canonical disposition |
| --- | --- |
| user / org / project / package entities | Already represented by generated `zed_*` entities in `src/rust-orm/entities` |
| package upload/download/license/embedding entities | Already represented by canonical generated entities |
| private-by-default package creation | `write::create_package` plus schema default and visibility trigger |
| 10-day / 50-download promotion policy | Shared SQL functions and trigger, surfaced by `VisibilityLimits` and `set_package_visibility` |
| Shared Auth / Supabase projection | `write::upsert_user_from_session` keyed by `(realm, UUID subject)`; duplicate `FederatedIdentity` is retired |
| organization role authorization | API-owned authorization plus opaque role-specific database principals; raw ORM authorization helpers are retired |
| upload lifecycle | `registry::register_package_upload` with canonical statuses, R2/S3/GCS/FS evidence, terminal-state validation, and no raw bytes or credentials |
| download ledger | `registry::record_package_download`; every row is a completed served event and the shared trigger owns counters |
| package licenses | `registry::add_package_license`, including package/version primary-scope replacement |
| package and entity text search | Existing `read::search_packages` plus canonical `registry::search_registry` across org/project/package/version rows |
| embedding writes | `registry::upsert_embedding` against JSONB arrays and content SHA-256 evidence |
| semantic search | `registry::semantic_search`, visibility scoped and computed from JSONB vectors without the prohibited pgvector extension |
| branch-owned migrations | Retired; `migrations` applies only the exact shared-definitions SQL revision |
| unprefixed tables | Retired; all operations address collision-safe `zed_*` tables |
| `search_document` / pgvector assumptions | Retired; neither exists in the canonical shared contract |
| raw `DatabaseConnection` / `DbErr` public API | Retired; consumers receive opaque contexts and public `OrmError` |

## Consumer migration

### Behavioral library users

Replace Git sources pointing at `zed-pkg/zed-lib` with
`zed-pkg/zed-lib-core` and select the target appropriate to the language.

### ORM users

Keep the Rust crate import:

```rust
use zed_orm_core::...;
```

Change the Git repository to `zed-pkg/zed-lib-core`; the crate lives at
`src/rust-orm` and keeps the `zed-orm-core` package name.

Default builds remain read-only. API servers opt into `read-write`; only the
discrete migration job opts into `migrate`.

## Predecessor repository cutover

The historical repositories now carry merged migration notices:

```text
zed-pkg/zed-lib#8       12071278e728100cc80b8ba60297ebff8e0914cb
zed-pkg/zed-orm-core#4  a78baf355e8ba3b98fa09780af9f67e8b7c12b85
```

Those notices preserve package/import compatibility, exact merge and salvage
commits, consumer instructions, and links back to this ledger. Neither
predecessor repository should originate new feature or release work.

Both repositories are ready to be archived as read-only historical entry
points. Archival must preserve all branches, tags, issues, pull requests, and
commit URLs; no predecessor history is deleted, force-pushed, or replaced with
an unrelated root commit.
