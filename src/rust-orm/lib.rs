//! # zed-orm-core
//!
//! Canonical SeaORM data plane for the `zed-pkg` registry — the merge of the
//! former `zed-lib` ORM crate and the `zed-orm-core` boundary crate.
//!
//! Three rules define this crate:
//!
//! 1. **The schema is not ours.** Every table lives in
//!    `pg-defs/schema/orgs/zed-pkg/registry.sql` in `k8s-libs-and-shared-defs`
//!    at the revision pinned in [`schema::SHARED_DEFS_REVISION`] and
//!    `shared-defs.lock.json`. [`migrations`] applies that reviewed SQL and
//!    authors none of its own.
//! 2. **Raw sessions do not escape.** Consumers receive an opaque
//!    [`ReadContext`] or [`WriteContext`] and call named operations in [`read`],
//!    [`write`], and the feature-gated [`invitations`] module. SeaORM entities
//!    and query builders stay private.
//! 3. **Writes are opt-in.** Default builds cannot compile a write symbol; API
//!    servers must enable `read-write`, and only the discrete migration job
//!    enables `migrate`. The feature split expresses intent — the authoritative
//!    control is the database principal, since Cargo features are additive
//!    across a dependency graph.
//!
//! ## Identity
//!
//! The registry and Shared Auth are separate data planes on separate RDS
//! instances. Supabase Auth is the identity provider; `shared-auth-server.rs`
//! verifies the Supabase JWT, owns the principal, and issues the session.
//! A principal maps to exactly one registry user through
//! `zed_users.shared_auth_subject` + `zed_users.auth_realm` — a cross-instance
//! reference with deliberately no foreign key, so [`write::upsert_user_from_session`]
//! is what keeps the two planes consistent.

#[cfg(not(feature = "read-only"))]
compile_error!("zed-orm-core requires the read-only feature; read-write includes it");

mod connection;
mod error;
mod policy;
pub mod read;
pub mod schema;

// Entities are public for the API server's own composite queries, but a
// consumer still needs a context from this crate to execute anything.
pub mod entities;
pub mod models;

#[cfg(feature = "read-write")]
pub mod invitations;
#[cfg(feature = "read-write")]
pub mod write;

#[cfg(feature = "migrate")]
pub mod migrations;

pub use connection::{
    connect_read_only, connect_read_only_with_policy, ConnectPolicy, ReadContext,
};
#[cfg(feature = "read-write")]
pub use connection::{connect_read_write, connect_read_write_with_policy, WriteContext};
pub use error::{OrmError, SQLSTATE_VISIBILITY_TOO_MANY_DOWNLOADS, SQLSTATE_VISIBILITY_TOO_OLD};
pub use policy::{PromotionRefusal, VisibilityLimits};
pub use schema::{
    qualified, ORG_SCHEMA, SHARED_DEFS_ORG_SLICE, SHARED_DEFS_REGISTRY_SEGMENT,
    SHARED_DEFS_REVISION, SHARED_DEFS_SEA_ORM_ADAPTER, TABLE_PREFIX,
};

/// Default consumers cannot import write symbols. This doctest is compiled only
/// for the default/read-only surface; all-feature API builds omit it.
#[cfg(not(feature = "read-write"))]
#[doc = r#"
```compile_fail
use zed_orm_core::{connect_read_write, invitations, write, WriteContext};
```
"#]
pub mod default_surface_compile_fail {}

/// Default consumers cannot reach the migration runner either.
#[cfg(not(feature = "migrate"))]
#[doc = r#"
```compile_fail
use zed_orm_core::migrations;
```
"#]
pub mod default_surface_migrate_compile_fail {}
