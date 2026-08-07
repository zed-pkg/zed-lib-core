//! Implementations of the zed-pkg contract defined in
//! [`zed-interfaces`](https://github.com/zed-pkg/zed-interfaces).
//!
//! The split is deliberate:
//!
//! * `zed-interfaces` is **shape** — the types, their serialization, and the
//!   validation that says whether a document is well-formed. Every service and
//!   client depends on it, so it must stay cheap to compile and free of
//!   opinion.
//! * `zed-lib` is **behavior** — the logic that composes those types into
//!   answers: resolution, planning, policy. It depends on `zed-interfaces` and
//!   is depended on by `zed-cli`, the servers, and the front ends.
//!
//! Behavior that lives in `zed-interfaces` today (version parsing, exclude
//! matching, ecosystem detection) moves here one module at a time; the
//! interface crate keeps the types and re-exports nothing. Until a module has
//! moved, do not copy it — depend on it.
//!
//! Rust is the first slice. Dart and TypeScript implementations of the same
//! behavior will live beside it under `src/`, verified against the shared
//! corpus in `conformance/`, so a front end and the CLI cannot disagree about
//! what a requirement resolves to.

pub mod resolve;

pub use resolve::{ResolveError, latest_stable, resolve_version};
