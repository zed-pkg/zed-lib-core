# zed-schema

This directory contains the immutable deployed PostgreSQL baseline for the
Zed registry during the dual-source transition. It publishes independently as
`zed-pkg/zed-schema`; it has no runtime package dependencies and does not
include the Rust ORM implementation.

`registry.sql` plus the immutable forward migrations are the only SQL that a
migration job may plan or apply today. The Drizzle and SeaORM files generated
elsewhere in this repository are non-authoritative parity evidence.

The target authority consists of co-equal, independently authored TypeSpec and
JSON Schema/OpenAPI peers plus one common PostgreSQL extension bundle. The
TypeSpec lane emits SQL, Protobuf/gRPC, wire-client, and ORM candidates; the
JSON Schema/OpenAPI lane separately emits SQL, interfaces, validators,
HTTP/write-client, and ORM candidates. Both must pass
source/catalog/Diesel/SeaORM/behavior/client parity before a jointly reviewed
desired SQL release replaces this baseline. Neither authored source is
generated from or canonical over the other.

Do not rewrite applied migrations or regenerate `registry.sql` in place merely
to adopt that target. Until every promotion and `declarative-migrations` gate in
`docs/ddl-first-schema-ownership.md` passes, this baseline remains the only
operational input and dual-source output remains candidate evidence.
