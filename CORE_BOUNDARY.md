# Zed core repository boundary

`zed-lib-core` is the shared, non-persistence package. It finalizes interfaces, validators, protocol-neutral rules, polyglot resolution behavior, and explicit `client/`, `server/`, `edge/`, and `isomorph/` surfaces.

TypeSpec and JSON Schema/OpenAPI remain independently authored in `zed-interfaces`. This repository consumes the same immutable source snapshots through two independent lanes and compares normalized interfaces, persistence, SQL/catalog, and ORM evidence. Normalized ORM IR is evidence only; executable Diesel/SeaORM code finalizes in the private `zed-orm-core` repository.

The historical `src/rust-orm` package is transition debt. It is removed from the Cargo workspace and excluded from every Zed publication by this change. Release tags remain blocked until its unique behavior is migrated and verified in `zed-orm-core`, after which the directory must be deleted from this repository.

Shared surfaces accept typed configuration inward. Client, edge, and isomorphic code never reads process environment values. A separate `zed-env` repository is justified only if multiple repositories or runtimes share one configuration schema; it would contain validation and classification only, never values or secrets.
