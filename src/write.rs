//! Write surface — API servers only (`read-write` feature).
//!
//! Mutation authorization, validation, invariants, and audit behavior
//! belong to the API server that calls into this module; the database
//! grants for web identities deny all DML in defense in depth.
