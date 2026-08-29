# DDL round-trip shadow artifacts

These files are generated from `src/rust-orm/sql/registry.sql` through a disposable PostgreSQL database. They prove ORM projection parity. They are **not migrations** and must never be applied to a database.
