# zed-schema

This directory contains the immutable deployed PostgreSQL P2 baseline for the
Zed registry during the dual-source transition. It publishes independently as
`zed-pkg/zed-schema`; it has no runtime package dependencies and does not
include the Rust ORM implementation.

`registry.sql` plus the immutable forward migrations are the only SQL that a
migration job may plan or apply today. The Drizzle and SeaORM files generated
elsewhere in this repository are non-authoritative parity evidence.

The target authority is an authored TypeSpec P0 canonical AST plus an
independently authored JSON Schema P1 secondary-primary source, with one common
PostgreSQL extension bundle. Both inputs emit candidates and must pass
source/catalog/Diesel/SeaORM/behavior/wire parity before a reviewed desired SQL
release replaces this baseline. A TypeSpec-emitted JSON Schema is diagnostic
output and must never overwrite P1.

Do not rewrite applied migrations or regenerate `registry.sql` in place merely
to adopt that target. Until every promotion and `declarative-migrations` gate in
`docs/ddl-first-schema-ownership.md` passes, this P2 baseline remains the only
operational input and dual-source output remains candidate evidence.
