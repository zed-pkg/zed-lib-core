# zed-orm-core

Shared **SeaORM** entity/query crate for the `zed-pkg` organization.

Both the web server and the API server read from the database, so ORM code
lives here once instead of being duplicated per service. Governed by the
org-wide policy in
[`zed-pkg/.github/SERVICE_AND_DATA_ARCHITECTURE.md`](https://github.com/zed-pkg/.github/blob/main/SERVICE_AND_DATA_ARCHITECTURE.md)
and its ORM addendum (`docs/ORM_CORE_LIBRARY.md`).

## Contract

| Consumer | Feature | Surface |
| --- | --- | --- |
| API server (Rust) | `read-write` | Full entity read/write surface |
| Web server | `read-only` (default) | Named, policy-aware query functions only |

- Schema definitions are **imported from [oresoftware/k8s-libs-and-shared-defs](https://github.com/oresoftware/k8s-libs-and-shared-defs)**, namespaced/segmented by GitHub org and project. This crate never defines an independent schema.
- **No migrations here.** The owning API server holds sole migration authority via [declarative-migrations](https://github.com/declarative-migrations); this crate is entities and queries only.
- Each release pins the exact shared-definition version/digest it was generated against; major version bumps are treated as schema events.
- Targets PostgreSQL and CockroachDB (postgres wire protocol) through SeaORM's `sqlx-postgres` backend; engine-specific behavior must be tested per engine.
