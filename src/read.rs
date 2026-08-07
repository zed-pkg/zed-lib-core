//! Read-only, policy-aware named query functions.
//!
//! Every function here must carry tenant/user scope and apply redaction —
//! this module is the web tier's entire view of the database. Prefer
//! `get_published_items_for_tenant(tenant_id)`-style contracts over
//! exposing entities or query builders.
