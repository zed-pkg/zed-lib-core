//! SeaORM entities for the `zed_*` registry tables.
//!
//! Every entity here mirrors exactly one table in the package-owned
//! `sql/registry.sql` (see [`crate::schema`]). The authored SQL is the source
//! of truth: SeaORM is regenerated from a disposable database, and a schema
//! change lands in this repository first.

pub mod api_token;
pub mod audit_log;
pub mod dependency_graph_artifact;
pub mod dependency_graph_edge;
pub mod entity_embedding;
pub mod org;
pub mod org_invitation;
pub mod org_member;
pub mod package;
pub mod package_download;
pub mod package_license;
pub mod package_upload;
pub mod package_version;
pub mod project;
pub mod project_invitation;
pub mod project_member;
pub mod user;
