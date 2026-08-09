# zed-orm-core

Canonical **SeaORM** boundary for the `zed-pkg` organization. It is the only repository that may publish the shared Zed ORM package; `zed-lib` may consume or re-export it temporarily but must not define a second authoritative ORM crate.

Governed by [`zed-pkg/.github/SERVICE_AND_DATA_ARCHITECTURE.md`](https://github.com/zed-pkg/.github/blob/main/SERVICE_AND_DATA_ARCHITECTURE.md).

## Contract

| Consumer | Feature | Public surface |
| --- | --- | --- |
| Web/default consumer | `read-only` (default) | `ReadContext`, role-aware connection, and named functions under `read` |
| API server | `read-write` | Adds `WriteContext` and named functions under `write` |

Raw SeaORM/SQLx connections, entity managers, query builders, and backend error types stay private. A default consumer cannot import `WriteContext`, `connect_read_write`, or the `write` module; a compile-fail doctest enforces that. Note this is an intent-and-ergonomics boundary, not a security one: Cargo feature resolution is additive, so any crate in a consumer's graph that enables `read-write` turns those symbols on. The authoritative control is the SELECT-only database role.

`connect_read_only` pins `search_path=zed_pkg`, sets `default_transaction_read_only=on` in the PostgreSQL startup packet, and verifies both settings before returning an opaque context. `connect_read_write` is compiled only with `read-write` and rejects a transaction-read-only session.

## Shared schema source

Schema definitions come from [`ORESoftware/k8s-libs-and-shared-defs`](https://github.com/ORESoftware/k8s-libs-and-shared-defs), never from independently authored entities here. [`shared-defs.lock.json`](shared-defs.lock.json) pins revision `c8bdc06d74746acc6439f9527ebd02697fdf028b`, organization slice `zed-pkg`, schema `zed_pkg`, and the generated Rust SeaORM adapter path.

Each release pins the exact shared-definition revision/digest it was generated against; a major version bump is treated as a schema event and participates in the expand/contract compatibility window. The crate targets PostgreSQL and CockroachDB (postgres wire protocol) through SeaORM's `sqlx-postgres` backend, but a shared codebase does not make the engines behave identically — engine-specific behavior, notably retryable serialization errors, must be tested per engine.

The connection and feature boundary is implemented now. Importing the generated Zed entity slice and replacing the generic connection-state reads with business-specific named queries remains a merge gate; do not expose the generated crate wholesale to consumers.

## Usage

Default web/read consumer:

```toml
zed-orm-core = { git = "https://github.com/zed-pkg/zed-orm-core.git", rev = "<merge-commit>" }
```

```rust,no_run
use zed_orm_core::{connect_read_only, read};

# async fn example() -> Result<(), zed_orm_core::OrmError> {
let context = connect_read_only("postgres://zed_web_ro@db/registry").await?;
read::ping(&context).await?;
# Ok(())
# }
```

API/write consumer:

```toml
zed-orm-core = {
  git = "https://github.com/zed-pkg/zed-orm-core.git",
  rev = "<merge-commit>",
  default-features = false,
  features = ["read-write"]
}
```

```rust,no_run
use zed_orm_core::{connect_read_write, write};

# async fn example() -> Result<(), zed_orm_core::OrmError> {
let context = connect_read_write("postgres://zed_api_rw@db/registry").await?;
write::ping(&context).await?;
# Ok(())
# }
```

## Migrations

There is no migration tooling in this crate. `zed-api-server` owns compatibility requirements, and a separate `declarative-migrations`/`dpm` release job applies reviewed DDL with the project-scoped migrator identity. Runtime API and web identities do not receive DDL rights.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo test --all-targets --all-features
cargo test --doc
```

A live denial probe is included but ignored by default because it performs an intentionally forbidden DDL statement against a disposable database:

```sh
ORM_CORE_TEST_DATABASE_URL='postgres://zed_web_ro@localhost/zed_test' \
  cargo test live_read_only_context_rejects_schema_ddl -- --ignored
```

Run that lane against both PostgreSQL and CockroachDB with a real SELECT-only web principal before releasing a consumer pin.
