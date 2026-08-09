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

The ORM files are adapters. They do not own migrations. In particular, Ent
models use `entsql.Skip()` and composite-key tables are emitted as `ent.View`
models so Ent cannot inject a synthetic key or mutate the schema.

## Interface ownership

`zed-interfaces` remains the sole owner of wire contracts. The lock file binds
persistence entities to existing public DTOs where a real API projection
exists:

- `Org` → `ClaimOrgResponse`
- `Package` → `PackageMetadata`
- `PackageVersion` → `VersionMetadata`
- `AuditLog` → `AuditLogResponse`
- `EntityEmbedding` → `EmbeddingUpsertRequest`

Memberships, invitations, tokens, upload attempts, and other internal records
remain internal persistence types. The generated descriptors may name those
records for storage and authorization routines, but they must not publish a
second API contract.

## Drift gates

`tools/check-schema-shadow.mjs` fails when any of these change without a
semantic reconciliation in the same commit:

1. the immutable `zed-interfaces` revision or schema-index blob;
2. the production SQL Git-blob identity;
3. any of the 15 SeaORM entity source blob identities;
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
node tools/check-schema-shadow.mjs
node --test tests/schema-shadow.test.mjs
```

Generation requires Node.js and `gofmt`; it has no npm dependency install step.

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
