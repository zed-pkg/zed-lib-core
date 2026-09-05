# Dual-source product schema ownership

## Decision and transition status

Each product organization owns its persistence model, product SQL, generated
ORM artifacts, and migration evidence. For Zed Pkg that owner is
`zed-pkg/zed-lib-core`; the former standalone `zed-orm-core` history has already
been merged here and must not be revived as a competing source.

The target has two co-equal, independently authored model authorities:

- **TypeSpec peer source.** It carries stable identities, references, wire
  contracts, persistence annotations, and emitter extension points, and emits
  its own SQL, Protobuf/proto3, gRPC, wire-client, and ORM candidates.
- **JSON Schema Draft 2020-12/OpenAPI peer source.** It is maintained
  separately and emits its own SQL, interfaces, language types, runtime
  validators, HTTP/write clients, and ORM candidates.

Neither source supplies canonical naming, conflict resolution, release
lineage, fallback semantics, or generated content for the other. Both sources
produce independent candidates, and an unexplained difference blocks
promotion. Optional cross-translations are diagnostic witnesses only; they
never feed SQL, Protobuf, OpenAPI, clients, ORM code, migration artifacts, or a
release.

This repository is still in the DDL-first transition phase. The checked-in
`src/rust-orm/sql/**` DDL and immutable forward migrations remain the deployed
legacy baseline until the dual-source emitters, parity gates, live-catalog
readback, and migration rehearsal are complete. Existing DDL must not be
silently regenerated or applied migrations rewritten merely to declare the new
architecture finished.

## Authority and projection model

| Layer | Direction | Target role |
| --- | --- | --- |
| TypeSpec peer | authored model -> SQL/IR/Protobuf/gRPC/wire/Diesel/SeaORM candidates | independent authority and release veto |
| JSON Schema/OpenAPI peer | separately authored model -> SQL/IR/interfaces/validators/HTTP clients/Diesel/SeaORM candidates | independent authority and release veto |
| PostgreSQL extension E | authored SQL fragments -> both candidates | semantics the common model cannot safely express: RLS, grants, functions, triggers, special checks, indexes, ownership, and ledger rules |
| Current `src/rust-orm/sql/**` | reviewed DDL -> PostgreSQL | immutable deployed baseline during transition; later a generated-and-reviewed desired release plus authored E |
| Diesel + `diesel-async` | normalized model -> primary Rust runtime | compile-time checked schema/models and named operations; schema diff may draft only the subset it represents |
| SeaORM CLI | scratch PostgreSQL -> secondary Rust entities | independent DB-first catalog readback; never the DDL source |
| Drizzle pull/export | scratch PostgreSQL <-> TypeScript shadow | additional lossy structural observer, not a migration authority |
| TypeSpec SQL/Protobuf/client/ORM emitters | TypeSpec peer -> generated candidates | independent TypeSpec-lane artifacts and compatibility evidence |
| JSON Schema/OpenAPI SQL/type/client/ORM emitters | JSON Schema/OpenAPI peer -> generated candidates | independent JSON-lane artifacts and compatibility evidence |
| `declarative-migrations` | desired release <-> live database | reviewed plan, shadow verification, apply, and convergence evidence |

The PostgreSQL extension bundle E is applied identically to both peer SQL
candidates. It must not contain two divergent copies of a table definition. If
a PostgreSQL feature cannot be represented by both models, the parity manifest
records the exact representation loss and the authored extension that restores
the required database behavior.

## Repository and Zed package boundary

The source repository exposes independently installable artifacts so consumers
do not need to know its directory layout:

- `zed-pkg/zed-schema`: immutable desired SQL, forward migrations, ownership
  manifest, source/candidate digests, and parity evidence for the migration
  planner;
- `zed-pkg/zed-orm-core`: opaque Diesel-primary/SeaORM-secondary runtime for
  web, API, and admin services;
- `zed-pkg/zed-infra`: organization-fixed target manifests and
  environment-specific DPM plan/apply orchestration; and
- root `zed-lib-core` targets: domain services and generation/certification
  tooling, without copied SQL in downstream applications.

The current nested manifests at `src/rust-orm/.zpkg.toml` and
`src/rust-orm/sql/.zpkg.toml` remain the publication boundaries during the
transition. One coordinated source release binds both peer sources, extension
E, desired SQL, migrations, ORM manifests, and evidence to the same source
commit, while each package retains its own immutable artifact digest.

For organizations that keep lib-core and orm-core in separate repositories,
the required dependency direction is:

```text
*-lib-core@schema-release
       +--------------------------+
       v                          v
*-orm-core@runtime-release   *-infra@migration-release
       |                     (DPM plan/apply jobs)
       v
web / api / admin consumers by explicit capability
```

`*-orm-core` consumes an exact lib-core release through Zed. Lib-core must not
depend back on orm-core merely to obtain its own model or desired SQL.
Interfaces may be shared independently, but an interfaces repository does not
own database behavior. In this merged Zed repository the same graph is
expressed through separate nested package manifests and release coordinates.

No consumer reads a GitHub raw URL, a submodule path, or
`k8s-libs-and-shared-defs/pg-defs/schema/orgs/...`. It resolves an immutable
`org/package@version` and verifies the published digest.

## What remains shared

`ORESoftware/k8s-libs-and-shared-defs` retains only genuinely cross-product
database material: cluster bootstrap conventions, shared role templates,
extension policy, generic observability hooks, and compatibility shims with
multiple owners. Product tables, functions, RLS policies, guard codes, seed
rules, and migration histories belong to Zed Pkg.

The old shared-definitions revision and blob hashes remain in
`shared-defs.lock.json` as historical import evidence. They answer where the
deployed baseline came from and protect existing ledger identities; they do
not create a current dependency or require new Zed schema work to land in the
shared repository first.

The central repository may later publish a generated fleet composition lock
that lists product package coordinates and digests. It may not vendor editable
copies of product SQL back into a second authority.

## Candidate generation and parity

The release pipeline builds two disposable databases from independent inputs:

```text
TypeSpec peer            -> SQL A + IR A + Diesel A + Protobuf/gRPC
JSON Schema/OpenAPI peer -> SQL B + IR B + Diesel B + interfaces/clients

SQL A + extension E -> scratch PostgreSQL A -> SeaORM A + catalog A
SQL B + extension E -> scratch PostgreSQL B -> SeaORM B + catalog B
```

Promotion requires normalized equivalence at five layers:

1. **source semantics:** entities, fields, identities, nullability, defaults,
   constraints, relationships, deletion behavior, enums, and annotations;
2. **database catalog:** tables, columns, types, sequences, keys, checks,
   indexes, functions, triggers, policies, grants, ownership, extensions, and
   publication membership;
3. **ORM surfaces:** Diesel manifests A/B and SeaORM manifests A/B after
   removing only documented generator-order noise;
4. **behavior:** guard SQLSTATEs, RLS allow/deny matrices, transactions,
   cascades, tenant/owner isolation, and invariant tests; and
5. **wire/client compatibility:** TypeSpec-lane Protobuf/gRPC/wire artifacts and
   JSON-Schema/OpenAPI-lane interfaces, validators, HTTP/write clients, plus
   field numbers, presence, encoding, and shared positive/negative fixtures.

Expected differences are classified as `equivalent`, `intentional-loss`,
`extension-owned`, `temporary-transition`, or `blocking`. Every non-equivalent
item needs a stable path, explanation, owner, test, and expiry/review condition.
Normalization may remove ordering and formatting noise; it must never erase a
default, constraint, RLS rule, function body, grant, identity, or wire-presence
difference.

Diesel is the primary Rust ORM because its explicit schema/model representation
supports the main compile-time checked data path. `diesel migration generate
--diff-schema` may propose structural migration fragments, but it does not
cover all PostgreSQL defaults and custom constraints, much less every RLS
policy, function, grant, or irreversible data step. Its output is a reviewed
candidate, not the production plan.

SeaORM remains deliberately useful as a secondary DB-first observer.
`sea-orm-cli` is run only after each candidate is applied to scratch
PostgreSQL. The generated entities test what the database actually accepted;
they do not define the database in reverse.

## Existing Zed certification during transition

`tools/ddl-first-orm-roundtrip.mjs` remains a fail-closed certification of the
current legacy DDL baseline while the new emitters are built:

1. it accepts only a loopback PostgreSQL database named with the
   `zed_ddl_roundtrip_` prefix and explicit write opt-in;
2. it refuses a non-empty `public` schema, applies `registry.sql`, and verifies
   the catalog plus `ZD001` through `ZD005` guards;
3. it generates SeaORM entities using the pinned CLI;
4. it pulls and exports a Drizzle representation; and
5. it compares deterministic generated evidence, allowing only explicitly
   locked representation loss.

The current desired state records 17 tables, 213 columns, 40 foreign keys, 109
checks, 58 non-constraint secondary indexes, 15 application triggers, and 8
`zed_*` functions. It records zero current `zed_*` RLS policies so that adding
one is a visible reviewed change rather than an assumption.

`tools/typespec-protobuf-parity.mjs` currently projects from the locked
persistence shadow. That is valuable transition evidence, not yet the target
peer-source architecture. Before promotion, TypeSpec and JSON Schema/OpenAPI
must become separately authored inputs and both must pass independence and
provenance audits. Stable Protobuf names and numbers remain locked, including
reserved removed fields. See `docs/typespec-protobuf-shadow.md` for the current
projection and its declared nullable, JSON, timestamp, and `int64` losses.

## Per-org declarative migration handoff

`zed-pkg/zed-infra` owns migration execution; `zed-pkg/zed-orm-core` does not.
The infra repository stores configuration and package pins, not product SQL.
Its non-secret `db/migration-targets.toml` fixes `organization = "zed-pkg"` and
declares only approved `canonical` and `auth` targets. Each target binds the
owned schema, exact Zed schema-release and DPM digests, PostgreSQL range,
direct/unpooled connection reference, separate read-only plan and schema-scoped
DDL role references, and advisory-lock scope. It cannot target another
organization and contains no DSN or credential.

Every organization exposes the same `db-plan`, `db-verify`, `db-apply`, and
`db-status` targets. They call a Rust wrapper whose root `.cli-flags.toml` is
the sole option contract and whose argv/environment boundary is parsed and
audited by `flags-2-env`. ores-sops/Fiducia inject credentials at runtime;
secrets are never accepted as flags.

A migration job receives:

- the `zed-pkg/zed-schema` version, artifact digest, source commit, desired SQL
  digest, and parity manifest digest;
- the destination database identity and engine version;
- a read-only diff credential; and
- explicit environment and tenant scope.

The job resolves the Zed artifact, verifies all digests, dumps a fresh live
catalog, and asks `declarative-migrations`/`dpm` for a plan. The plan artifact is
bound to the package digest, live database fingerprint, and planner version.
The separate apply job requires the reviewed plan digest, rejects a stale live
fingerprint, uses the dedicated migrator principal, verifies convergence, and
records evidence. It obtains a lock for `(organization, database plane,
schema)`, then requires an empty residual diff, real-role/RLS tests, and a
published execution receipt before releasing that lock.

Web/API/admin servers never receive DDL credentials and never migrate on
startup. The diff job never receives apply credentials. Data backfills,
ownership/role changes, irreversible transforms, and features outside the
planner's catalog coverage are explicit companion steps; zero structural drift
does not prove them correct. ORM core may expose a read-only
compatibility/readiness check bound to the expected schema-release digest, but
no DPM wrapper.

Under DEN-3146, Zed Pkg temporarily uses its own schema in each shared
`oresoftware` Supabase `canonical` and `auth` database plane. Custom product
objects never enter Supabase-managed `auth`, `storage`, or `realtime` schemas.
The apply role is not superuser, has no `BYPASSRLS`, and has no cross-org
grants. A package-scoped DPM plan is permitted only after dependency-complete
object-selection tests prove it cannot inspect or mutate another org's objects;
otherwise use a full physical-database composition and lock. Moving to the
future per-org Supabase project changes typed RuntimeConfig and infra overlays
only. The same logical target contract applies to the mapped Neon/Lakebase org,
with production-derived branch rehearsal and direct/unpooled migration
connections.

## Fleet migration sequence

Move one organization at a time:

1. Inventory product SQL, live objects, ledgers, database topology, writers,
   runtime roles, and every shared-definition consumer.
2. Import exact deployed SQL and hashes as historical provenance; do not
   rewrite applied migrations or change namespaces during ownership transfer.
3. Establish co-equal authored TypeSpec and JSON Schema/OpenAPI peers,
   extension E, stable IDs, and the expected-divergence registry.
4. Generate A/B candidates, apply each to disposable PostgreSQL, generate
   Diesel and SeaORM surfaces, and pass source/catalog/ORM/behavior/wire parity.
5. Publish candidate packages, test isolated Zed installation, and rehearse an
   empty build plus upgrades from every deployed ledger version.
6. Run `dpm` plan-only against staging and production with a read-only role;
   review every expected and unexpected delta.
7. Publish immutable schema and ORM releases, then pin migration and runtime
   consumers to matching source/evidence digests.
8. Replace the shared repository's product SQL with a generated ownership
   pointer only after every consumer has moved and the live database converges.
9. Remove compatibility aliases in a later reviewed major release.

## Release gates for this repository

The dual-source target is not complete until:

- TypeSpec and JSON Schema/OpenAPI are independently authored, co-equal, and
  protected from generator overwrite;
- the A/B databases pass all five parity layers with reviewed divergences;
- Zed manifests validate and isolated consumers install both packages by
  immutable digest;
- Diesel-primary named operations and SeaORM-secondary checks remain behind
  the opaque capability boundary;
- `dpm` plan, shadow verification, apply, and convergence evidence pass for the
  deployed ledger lineage; and
- server repositories no longer import Zed product SQL from the shared repo.

Until those gates pass, current DDL remains the immutable deployed authority,
and all generated dual-source output is candidate evidence only.

## References

- [Diesel schema-diff migration generation](https://diesel.rs/news/2_1_0_release.html)
- [SeaORM database-to-entity generation](https://www.sea-ql.org/SeaORM/docs/0.12.x/generate-entity/sea-orm-cli/)
- [TypeSpec custom emitters](https://typespec.io/docs/extending-typespec/emitters-basics/)
- [Declarative Migrations](https://github.com/declarative-migrations/declarative-postgres-migrate.rs)
