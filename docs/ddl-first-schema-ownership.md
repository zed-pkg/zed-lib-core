# DDL-first product schema ownership

## Decision

Each product organization owns its database schema in exactly one Zed package.
The owner is the product's `*-orm-core` repository when that repository exists;
otherwise it is a nested package beside the ORM implementation in `*-lib-core`.
`zed-pkg/zed-lib-core` uses the second form because the former `zed-orm-core`
history has already been merged here.

The authored PostgreSQL DDL and immutable forward migrations are the migration
authority. SeaORM and Drizzle are projections with different jobs:

| Layer | Direction | Authority | Purpose |
| --- | --- | --- | --- |
| `src/rust-orm/sql/**` | authored files -> PostgreSQL | yes | tables, constraints, functions, triggers, grants, ownership, RLS, guard SQLSTATEs, and ledger-safe forward changes |
| SeaORM CLI | PostgreSQL -> Rust entities | no | compile-time Rust query model used behind the opaque ORM boundary |
| Drizzle Kit pull | PostgreSQL -> TypeScript schema | no | independent structural view of the applied DDL |
| Drizzle Kit export | TypeScript schema -> SQL | no | proves that an ORM in the toolchain can emit SQL and exposes what that model loses |
| TypeSpec | locked persistence shadow -> TypeSpec | no | one generated contract surface for standard JSON Schema and Protobuf emitters |
| TypeSpec JSON Schema + Protobuf | TypeSpec -> cross-runtime artifacts | no | checks 17 messages, 213 fields, stable wire numbers, presence, and declared representation loss |
| declarative-migrations | released desired DDL <-> live database | plan authority only | produces the reviewed drift plan; a separate job applies an approved plan |

This deliberately does not promote `schema/persistence.schema.json` to
model-first authority. That shadow remains useful and round-trip capable, but a
future promotion is a separate project with its own semantic-completeness gate.

## Repository boundary

The source repository exposes two independently installable Zed packages, so
consumers do not have to know repository layout:

- `src/rust-orm/.zpkg.toml` -> `zed-pkg/zed-orm-core`: the opaque SeaORM/query
  boundary used by web, API, and admin servers. Default features are read-only;
  writes and migrations remain separately gated.
- `src/rust-orm/sql/.zpkg.toml` -> `zed-pkg/zed-schema`: only authored DDL,
  immutable forward migrations, and package evidence used by the migration
  planner/job.

The ORM and schema are intentionally nested standalone packages rather than
root polyglot targets. Zed target fan-out currently preserves root dependencies
and the root smoke test. Here that would give a data-only schema artifact an
unnecessary `zed-interfaces` dependency and would give both artifacts a
conformance-corpus smoke test for files they do not contain. Separate manifests
let the ORM retain its real interface dependency while the schema remains
dependency-free, and each package tests the files and behavior actually present
in its installed artifact.

No consumer reads a GitHub raw URL, a submodule path, or
`k8s-libs-and-shared-defs/pg-defs/schema/orgs/...`. It resolves an immutable
`org/package@version` through Zed. One coordinated source release therefore
binds the Rust entities, authored DDL, forward migrations, and generated parity
evidence to the same source commit while each package keeps its own artifact
digest.

For organizations that keep `*-lib-core` and `*-orm-core` separate, use this
one-way graph:

```text
*-interfaces
      ^
      |
*-orm-core  -- nested <prefix>-orm-core + <prefix>-schema packages
      ^
      |
*-lib-core  -- domain services, no copied DDL
      ^
      |
web/api/admin/migration jobs
```

`*-lib-core` may depend on `*-orm-core` through Zed. The inverse edge is
forbidden. If the repositories are merged, as they are here, the same logical
boundary is expressed as independent ORM and schema manifests in the same
source repository.

## What remains shared

`ORESoftware/k8s-libs-and-shared-defs` should retain only genuinely
cross-product database material: cluster bootstrap conventions, common role
templates, extension policy, generic observability hooks, and compatibility
shims that have multiple owners. Product tables, product functions, product RLS
policies, product guard codes, and product migration histories belong to their
organization package.

The old Zed shared-definitions revision and blob hashes remain in
`shared-defs.lock.json` as historical import evidence. They preserve the answer
to “where did this exact deployed SQL come from?” and protect existing ledger
identities. They no longer create a package dependency or require future schema
changes to land in the shared repository first.

## Round-trip certification

`tools/ddl-first-orm-roundtrip.mjs` performs a fail-closed certification against
an empty disposable PostgreSQL database:

1. It accepts only a loopback PostgreSQL URL whose database name begins with
   `zed_ddl_roundtrip_`, plus the explicit `DDL_ROUNDTRIP_ALLOW_WRITE=1` opt-in.
2. It refuses a non-empty `public` schema, then applies only the authored
   `registry.sql` with `ON_ERROR_STOP`.
3. It verifies the live catalog and the `ZD001` through `ZD005` guard functions.
4. It generates compact SeaORM entities with exactly `sea-orm-cli 1.1.19`.
5. It introspects a Drizzle schema and exports SQL from that schema.
6. It fails unless the one current Drizzle empty-string introspection defect is
   the exact `zed_packages.repo_url` expression. The DDL is never weakened to
   suit the generator.
7. It sorts named metadata and table declarations and removes Drizzle's
   catalog-order-dependent implicit B-tree operator-class annotations. This is
   allowed only while authored DDL declares no explicit operator class; adding
   one fails closed until the shadow generator learns to preserve it.
8. It compares deterministic output with `generated/ddl-roundtrip/**`.

The generated Drizzle SQL intentionally lacks PostgreSQL functions, triggers,
and `ZD001`-`ZD005`; that visible loss is a proof that it is not a replacement
for authored DDL. RLS policies, grants, ownership, extensions, irreversible
data transforms, and migration-ledger identity remain authored semantics even
if a future Drizzle version can represent some of them.

The current Zed desired state contains 17 tables, 213 columns, 40 foreign keys,
109 checks, 58 non-constraint secondary indexes, 15 application triggers, and
8 `zed_*` functions. It currently has no `zed_*` RLS policies; the catalog
profile records that zero so adding policies becomes an explicit reviewed
change rather than an assumption.

`tools/typespec-protobuf-parity.mjs` adds a second fan-out cross-check from the
locked persistence shadow. The pinned official TypeSpec emitters generate Draft
2020-12 JSON Schema and proto3, while an independent parser verifies every
message, field, type, presence label, and field number. The compatibility lock
reserves removed Protobuf names and numbers. Its manifest explicitly records
nullable-as-optional, opaque JSON bytes, RFC 3339 timestamp strings, and JSON
decimal-string `int64` transformations; none of those projections can weaken or
replace the authored PostgreSQL semantics. See
`docs/typespec-protobuf-shadow.md`.

## declarative-migrations handoff

The deployment repository should contain configuration, not product SQL. A
migration job receives these immutable inputs:

- schema package coordinate (for Zed, `zed-pkg/zed-schema`), version, artifact
  digest, and source commit;
- destination database identity and engine version;
- a read-only diff credential;
- explicit environment and tenant scope.

The planner resolves the Zed artifact, verifies its digest, and asks
declarative-migrations to compare the desired DDL with the live catalog. It
publishes a plan artifact keyed by the package digest, database fingerprint,
and planner version. The apply job is separate: it requires the reviewed plan
digest, rechecks the live fingerprint to reject stale plans, uses a migrator
principal, and records the result. Web/API/admin servers never receive DDL
credentials, and the diff job never receives apply credentials.

This keeps coupling at three stable contracts:

1. Zed package identity and immutable artifact resolution;
2. the declarative-migrations plan/apply interface;
3. PostgreSQL catalog semantics.

Neither the planner nor the servers know the product repository's directory
layout.

## Fleet migration sequence

Apply the move organization by organization, never as one fleet-wide pointer
flip:

1. Inventory every SQL file, migration ledger, live database, writer, and
   current shared-definition consumer. Classify shared platform SQL separately
   from product SQL.
2. Name one owner: `*-orm-core`, or a dependency-free nested schema package in
   `*-lib-core`. Reject any domain schema that appears in both.
3. Import the exact reviewed files without rewriting applied migrations. Record
   the old repository revision and blob hashes as historical provenance.
4. Add disposable-Postgres generation for SeaORM and the companion ORM, plus
   TypeSpec JSON Schema/Protobuf projection checks and catalog/guard/RLS tests.
   Generated SQL and wire artifacts remain review evidence.
5. Publish a real immutable Zed package and test it from an isolated consumer,
   including target path resolution and execution—not only source tests.
6. Run declarative-migrations in plan-only mode against staging and production
   with a read-only principal. Zero unexpected drift is the promotion gate.
7. Pin web, API, and admin jobs to the released ORM package and the migration
   job to the schema package from the same source release. Only the migration
   job receives migration rights.
8. Replace the shared repository's product SQL with a tombstone/ownership index
   pointing to the package coordinate and last historical blob. Delete the
   duplicate only after every consumer pin and deployment configuration has
   moved.
9. Rehearse an empty build, an upgrade from every deployed ledger version, and
   rollback/restore. Then remove compatibility aliases in a later major
   release.

## Release gates for this repository

This repository is not consumable merely because the source branch is green.
Both package manifests must validate with the fleet's Zed CLI, each package must
be published to a working registry with an immutable digest/source commit, and
isolated consumers must install both `zed-pkg/zed-orm-core` and
`zed-pkg/zed-schema`. Only after that gate may server repositories replace
their blocked ORM dependency coordinates or the shared repository remove its
historical Zed SQL copy.
