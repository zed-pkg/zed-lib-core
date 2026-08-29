# zed-schema

This directory is the authored PostgreSQL schema authority for the Zed
registry. It publishes independently as `zed-pkg/zed-schema`; it has no runtime
package dependencies and does not include the Rust ORM implementation.

`registry.sql` plus the immutable forward migrations are the only SQL that a
migration job may plan or apply. The Drizzle and SeaORM files generated
elsewhere in this repository are non-authoritative parity evidence.
