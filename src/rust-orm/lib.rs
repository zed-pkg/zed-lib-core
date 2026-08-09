//! Canonical SeaORM data plane for the `zed-pkg` registry.
//!
//! `zed-lib` owns the durable registry schema, migrations, entities, and named
//! operations used by the API server, the MASH web server, migration jobs, and
//! background workers. Services import this crate instead of copying SeaORM
//! entities or constructing ad-hoc queries.
//!
//! The registry and Shared Auth remain separate data planes:
//!
//! - this crate owns registry authorization and product data (`users`, `org`,
//!   `projects`, `package`, memberships, invitations, and package settings);
//! - Shared Auth owns authentication ceremonies and revocable sessions in its
//!   customer-auth RDS instance;
//! - a Shared Auth subject is mapped to exactly one registry user through
//!   `users.shared_auth_subject`.

mod connect;
pub mod entities;
pub mod migrations;
pub mod models;
pub mod queries;
mod schema;

pub use connect::{DbRole, apply_role, assert_read_only, connect};
pub use migrations::{ACCOUNT_CONSOLE_MIGRATION, MigrationReport, migrate};
pub use schema::{REGISTRY_SCHEMA, qualified};

pub use sea_orm;
