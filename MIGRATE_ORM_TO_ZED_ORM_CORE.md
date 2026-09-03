# Blocking migration: remove `src/rust-orm`

The nested ORM package predates `zed-pkg/zed-orm-core` and violates the current repository boundary. This PR prevents it from building or publishing as lib-core, but does not delete code before proving feature parity.

Before a zed-lib-core release:

1. Inventory every exported entity, named query, policy, publication, invitation, account, graph, license, search, migration, and schema behavior in `src/rust-orm`.
2. Implement or migrate the still-required runtime behavior into private `zed-orm-core` without exposing raw connections or application-startup migrations.
3. Generate Diesel and SeaORM views independently from the accepted catalog and run shared positive/negative fixtures.
4. Pin the same `zed-interfaces` and `zed-lib-core` revisions through Zed and native metadata.
5. Produce complete TypeSpec/JSON Schema evidence and `artifacts/agreement.lock` in both repositories.
6. Delete `src/rust-orm` and its nested package metadata from this repository.

No release waiver may retain executable ORM code in lib-core.
