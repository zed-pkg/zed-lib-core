//! Implementations of the zed-pkg contract defined in
//! [`zed-interfaces`](https://github.com/zed-pkg/zed-interfaces).
//!
//! `zed-lib` owns behavior: resolution, planning, policy, and the canonical
//! distributed-lock identities used by registry services. Local process locks
//! remain in `zed-lock`; cross-host/fenced coordination uses the shared
//! `ores-locks-and-leases` package declared by this repository's zed manifest.

pub mod locks;
pub mod namespace_plan;
pub mod resolve;

pub use namespace_plan::plan_registry_namespaces;
pub use resolve::{ResolveError, latest_stable, requirement_matches, resolve_version};
