# Zed core repository boundary

`zed-lib-core` is the shared, non-persistence package. It finalizes interfaces, validators, protocol-neutral rules, polyglot resolution behavior, and explicit `client/`, `server/`, `edge/`, and `isomorph/` surfaces.

TypeSpec and JSON Schema/OpenAPI remain independently authored in `zed-interfaces`. This repository consumes the same immutable source snapshots through two independent lanes and compares normalized interfaces, persistence, SQL/catalog, and ORM evidence. Normalized ORM IR is evidence only; executable Diesel/SeaORM code finalizes in the private `zed-orm-core` repository.

The historical `src/rust-orm` package is transition debt. The root Zed publication excludes the complete tree. It temporarily remains a Cargo workspace member so the certified predecessor build and lockfile continue to verify while behavior is migrated. The strict boundary/release gate fails while the directory remains; after feature parity is proved in `zed-orm-core`, the directory and nested package metadata must be deleted here.

Shared surfaces accept typed configuration inward. Client, edge, and isomorphic code never reads process environment values. A separate `zed-env` repository is justified only if multiple repositories or runtimes share one configuration schema; it would contain validation and classification only, never values or secrets.
