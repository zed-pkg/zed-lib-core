# JSON Schema ORM shadow contract

`zed-lib-core` already owns the live Zed registry data plane through the checked-in
SeaORM entities and `src/rust-orm/sql/registry.sql`. This directory adds the
cross-language contract requested for the wider `*-lib-core` fleet **without
rewriting that history**.

## Authority model

| Layer | Current authority |
| --- | --- |
| Shared wire/API DTOs | `zed-pkg/zed-interfaces` at the immutable revision in `schema/interfaces.lock.json` |
| Production PostgreSQL schema, triggers, indexes, grants, and migration identity | `src/rust-orm/sql/registry.sql` plus the existing migration ledger |
| Existing Rust persistence behavior | `src/rust-orm/entities/**`, `account.rs`, `publication.rs`, `read.rs`, and `write.rs` |
| Cross-language ORM adapters | `schema/persistence.schema.json` → `generated/schema-orm-shadow/**` |
| Migration authority after a future promotion | Not assigned; requires the promotion gate below |

The JSON Schema is an **imported shadow contract**. Its generated SQL is a
review artifact. It must not be applied to local, CI, staging, RDS, Supabase, or
production databases.

## Generated adapters

The same JSON Schema deterministically emits:

- Rust: SeaORM
- Node.js/TypeScript: Drizzle, Prisma, and TypeORM
- Go: GORM and Ent
- Dart: Drift and Stormberry
- PostgreSQL and SQLite shadow DDL
- shared entity descriptors and stable identity metadata for Rust, TypeScript,
  Go, and Dart

The same locked shadow also generates TypeSpec, Draft 2020-12 JSON Schema, and
proto3 under `generated/schema-contracts/**`. That separate fan-out is checked
by `tools/typespec-protobuf-parity.mjs`; its stable field-number and declared
representation-loss rules are documented in `docs/typespec-protobuf-shadow.md`.

The ORM files are adapters. They do not own migrations. In particular, Ent
models use `entsql.Skip()` and composite-key tables are emitted as `ent.View`
models so Ent cannot inject a synthetic key or mutate the schema.

The relationship projection is intentionally incomplete: production SQL has
40 foreign keys and 40 explicit `ON DELETE` actions, while the current JSON
Schema models 19 relationships and no referential actions. The other 21
foreign keys and all deletion behavior remain owned by `registry.sql`; the
generated shadow DDL uses dialect defaults and must not be used as a semantic
substitute. `schema/import.lock.json` pins these counts so the gap cannot grow
or be described as complete accidentally.

## Interface ownership

`zed-interfaces` remains the sole owner of wire contracts. The lock file binds
persistence entities to existing public DTOs where a real API projection
exists:

- `Org` → `ClaimOrgResponse`
- `Package` → `PackageMetadata`
- `PackageVersion` → `VersionMetadata`
- `AuditLog` → `AuditLogResponse`
- `EntityEmbedding` → `EmbeddingUpsertRequest`

Memberships, invitations, tokens, upload attempts, immutable dependency-graph
artifacts, normalized graph edges, and other internal records remain internal
persistence types. The generated descriptors may name those records for
storage and authorization routines, but they must not publish a second API
contract.

## Drift gates

`tools/check-schema-shadow.mjs` fails when any of these change without a
semantic reconciliation in the same commit:

1. the immutable `zed-interfaces` revision, schema-index blob, bound schema
   paths, and bound schema titles;
2. the production SQL Git-blob identity;
3. any of the 17 SeaORM entity source blob identities;
4. a SeaORM table name, field, Rust type, or nullability;
5. a JSON Schema table or column missing from production SQL;
6. the isolated `generated/schema-orm-shadow` output path; or
7. the explicit list of still-unmodeled production features.

This is intentionally stricter than choosing one side of a merge. When a live
entity changes, the reviewer must understand the SQL migration, SeaORM
behavior, public DTO projection, and every generated adapter before refreshing
the lock.

## Commands

```bash
node tools/schema-shadow-codegen.mjs
node tools/schema-shadow-codegen.mjs --check
node tools/check-schema-shadow.mjs . ../zed-interfaces
node --test tests/schema-shadow.test.mjs
npm --prefix schema-tooling run contracts:check
node --test tests/typespec-protobuf-parity.test.mjs
```

Generation requires Node.js and `gofmt`; it has no npm dependency install step.
The drift check also requires a sibling checkout of `zed-pkg/zed-interfaces`
containing the locked revision. CI fetches that exact revision independently.

## Promotion gate

A later PR may promote JSON Schema/generated SQL to migration authority only
after all of the following are true:

1. Every default, check constraint, unique/partial/expression/search index,
   trigger, function, grant, ownership rule, RLS policy, and extension in
   `registry.sql` is modeled or explicitly delegated without loss.
2. Generated PostgreSQL DDL matches the live schema structurally and
   semantically, including deletion/update actions and trigger behavior.
3. An empty database can be built from the promoted migration chain and passes
   the complete Rust/Node/Go/Dart conformance suite.
4. A snapshot of representative production-shaped data can be migrated forward
   and rolled back or restored without loss.
5. Existing migration ledger identity remains stable; no applied migration is
   edited in place.
6. Publication, download counters, audit append-only behavior, invitation
   security, owner isolation, and Shared Auth subject handling pass real
   PostgreSQL integration tests.
7. The promotion is an explicit, separately reviewed PR that changes
   `authorityMode`; it cannot happen as a side effect of ordinary codegen.

Until then, the word **SHADOW ONLY** in every generated SQL file is a hard
operational rule.
