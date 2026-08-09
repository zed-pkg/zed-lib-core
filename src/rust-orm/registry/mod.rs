//! Canonical registry data-plane operations salvaged from the predecessor
//! `zed-lib` registry branch.
//!
//! Reads accept an opaque [`crate::ReadContext`] plus explicit visibility scope.
//! Writes are compiled only with `read-write` and accept [`crate::WriteContext`].
//! The API tier owns authentication and authorization; this module owns
//! persistence validation, transactional invariants, and source redaction.

mod search;
mod validation;

pub use search::{EmbeddingInput, RegistrySearchHit, search_registry, semantic_search};
#[cfg(feature = "read-write")]
pub use search::upsert_embedding;

#[cfg(feature = "read-write")]
mod artifacts;
#[cfg(feature = "read-write")]
mod licenses;

#[cfg(feature = "read-write")]
pub use artifacts::{
    PackageDownloadInput, PackageUploadInput, record_package_download, register_package_upload,
};
#[cfg(feature = "read-write")]
pub use licenses::{PackageLicenseInput, add_package_license};
