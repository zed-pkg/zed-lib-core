//! # zed-orm-core
//!
//! Shared SeaORM entity and query layer for the `zed-pkg` organization.
//!
//! Schema definitions are imported from
//! `oresoftware/k8s-libs-and-shared-defs`, namespaced by GitHub org and
//! project per the org-wide service & data architecture policy
//! (`zed-pkg/.github/SERVICE_AND_DATA_ARCHITECTURE.md`). This crate never
//! defines an independent schema, and it carries no migration tooling —
//! migrations belong exclusively to the owning API server via
//! `declarative-migrations`.
//!
//! ## Consumer contract
//!
//! - **API servers** enable the `read-write` feature for the full surface.
//! - **Web servers** use the default `read-only` feature and get named,
//!   policy-aware query functions only. No raw `DatabaseConnection`,
//!   unrestricted query builder, or entity manager is exported to
//!   request handlers, and the web tier connects with its SELECT-only
//!   database identity.

pub mod read;

#[cfg(feature = "read-write")]
pub mod write;
