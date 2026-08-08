//! # zed-orm-core
//!
//! Canonical, opaque SeaORM boundary for the `zed-pkg` organization.
//!
//! The crate consumes the Zed schema slice generated from
//! `ORESoftware/k8s-libs-and-shared-defs`; it does not own migrations or expose
//! raw ORM sessions. Web/default consumers receive only [`ReadContext`] and
//! named functions under [`read`]. API consumers must explicitly enable the
//! `read-write` feature to compile [`WriteContext`] and [`write`].

#[cfg(not(feature = "read-only"))]
compile_error!("zed-orm-core requires the read-only feature; read-write includes it");

mod connection;
mod error;
pub mod read;
mod schema;

#[cfg(feature = "read-write")]
pub mod write;

pub use connection::{
    connect_read_only, connect_read_only_with_policy, ConnectPolicy, ReadContext,
};
#[cfg(feature = "read-write")]
pub use connection::{connect_read_write, connect_read_write_with_policy, WriteContext};
pub use error::OrmError;
pub use schema::{
    ORG_SCHEMA, SHARED_DEFS_ORG_SLICE, SHARED_DEFS_REVISION, SHARED_DEFS_SEA_ORM_ADAPTER,
};

/// Default consumers cannot import write symbols. This doctest is compiled only
/// for the default/read-only surface; all-feature API builds omit it.
#[cfg(not(feature = "read-write"))]
#[doc = r#"
```compile_fail
use zed_orm_core::{WriteContext, connect_read_write, write};
```
"#]
pub mod default_surface_compile_fail {}
