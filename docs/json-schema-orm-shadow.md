# JSON Schema transition and P1 persistence contract

`zed-lib-core` already owns the live Zed registry data plane through the
checked-in SeaORM entities and `src/rust-orm/sql/registry.sql`. This directory
currently adds an imported cross-language shadow **without rewriting that
history**. The target architecture promotes a reviewed, independently maintained
JSON Schema tree to P1 secondary-primary status alongside an authored TypeSpec
P0 canonical AST; the current import is not yet that P1.

## Authority model

| Layer | Current transition authority |
| --- | --- |
| Shared wire/API DTOs | `zed-pkg/zed-interfaces` at the immutable revision in `schema/interfaces.lock.json` |
| Production PostgreSQL schema, triggers, indexes, grants, and migration identity | `src/rust-orm/sql/registry.sql` plus the existing migration ledger |
| Existing Rust persistence behavior | `src/rust-orm/entities/**`, `account.rs`, `publication.rs`, `read.rs`, and `write.rs` |
| Cross-language ORM adapters | `schema/persistence.schema.json` → `generated/schema-orm-shadow/**` |
| Target dual-source authority | Not active; requires both P0 and P1 promotion gates below |

The current JSON Schema is an **imported shadow contract**. Its generated SQL is
a review artifact. It must not be applied to local, CI, staging, RDS, Supabase,
or production databases.

In the target state:

- TypeSpec P0 is the authored canonical AST for stable identity, naming,
  relationships, annotations, emitter metadata, and wire projection;
- JSON Schema P1 is independently authored, protected from emitter overwrite,
  and may veto a release when normalized semantics disagree; and
- the TypeSpec-emitted JSON Schema is stored at a separate generated path and
  compared with P1 as diagnostic evidence.

P0 and P1 each emit SQL/IR/ORM/wire candidates. Neither candidate becomes
desired SQL until both scratch-database catalogs, Diesel and SeaORM manifests,
behavior tests, wire contracts, and reviewed expected divergences agree.

## Generated adapters

The same JSON Schema deterministically emits:

- Rust: SeaORM
- Node.js/TypeScript: Drizzle, Prisma, and TypeORM
- Go: GORM and Ent
- Dart: Drift and Stormberry
- PostgreSQL and SQLite shadow DDL
- shared entity descriptors and stable identity metadata for Rust, TypeScript,
  Go, and Dart

The same locked shadow currently also generates TypeSpec, Draft 2020-12 JSON
Schema, and proto3 under `generated/schema-contracts/**`. That legacy fan-out is
checked by `tools/typespec-protobuf-parity.mjs`; its stable field-number and
declared representation-loss rules are documented in
`docs/typespec-protobuf-shadow.md`. Target P0 reverses this direction: TypeSpec
becomes authored input, while its emitted JSON Schema remains distinct from P1.

The ORM files are adapters. They do not own migrations. In particular, Ent
models use `entsql.Skip()` and composite-key tables are emitted as `ent.View`
models so Ent cannot inject a synthetic key or mutate the schema.

The current relationship projection is intentionally incomplete: production SQL has
40 foreign keys and 40 explicit `ON DELETE` actions, while the current JSON
Schema models 19 relationships and no referential actions. The other 21
foreign keys and all deletion behavior remain owned by the deployed
`registry.sql` baseline; the generated shadow DDL uses dialect defaults and
must not be used as a semantic substitute. `schema/import.lock.json` pins these
counts so the gap cannot grow or be described as complete accidentally. P1
promotion requires closing or explicitly extension-owning every one of these
differences.

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

## P1 and dual-source promotion gate

A later, explicit PR may promote the JSON Schema tree from imported shadow to
independently authored P1 secondary-primary status only after all of the
following are true:

1. The tree has an independence/provenance audit, protected authored path, and
   review process that prevents TypeSpec or another generator from overwriting
   it.
2. An authored TypeSpec P0 exists with stable cross-source IDs, and a normalized
   P0/P1 source comparison has no unexplained differences.
3. Every default, check constraint, unique/partial/expression/search index,
   trigger, function, grant, ownership rule, RLS policy, and extension in
   `registry.sql` is modeled or explicitly delegated without loss.
4. P0 SQL A and P1 SQL B, after the identical PostgreSQL extension bundle, build
   disposable databases whose normalized catalogs and behavior match the live
   contract.
5. Diesel A/B and SeaORM A/B manifests agree after documented normalization;
   generator output does not escape the opaque runtime boundary.
6. Generated PostgreSQL DDL matches the live schema structurally and
   semantically, including deletion/update actions and trigger behavior.
7. An empty database can be built from the promoted migration chain and passes
   the complete Rust/Node/Go/Dart conformance suite.
8. A snapshot of representative production-shaped data can be migrated forward
   and rolled back or restored without loss.
9. Existing migration ledger identity remains stable; no applied migration is
   edited in place.
10. Publication, download counters, audit append-only behavior, invitation
   security, owner isolation, and Shared Auth subject handling pass real
   PostgreSQL integration tests.
11. A read-only `declarative-migrations` plan against the live catalog is fully
    reviewed, shadow-verified, and bound to immutable Zed/evidence digests.
12. The promotion is an explicit, separately reviewed PR that changes
    `authorityMode`; it cannot happen as a side effect of ordinary codegen.

Until then, the word **SHADOW ONLY** in every current generated SQL file is a
hard operational rule. After promotion, only the combined, reviewed P0/P1
desired release—not an arbitrary emitter output—may enter the migration job.
