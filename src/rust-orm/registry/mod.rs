//! Canonical registry data-plane operations salvaged from the predecessor
//! `zed-lib` registry branch.
//!
//! Reads accept an opaque [`crate::ReadContext`] plus explicit visibility scope.
//! Writes are compiled only with `read-write` and accept [`crate::WriteContext`].
//! The API tier owns authentication and authorization; this module owns
//! persistence validation, transactional invariants, and source redaction.

mod search;
mod validation;

mod graphs;

pub use graphs::{
    dependency_graph_by_digest, incoming_dependency_edges, latest_dependency_graph_for_root,
    outgoing_dependency_edges, DependencyGraphCoordinate, DependencyGraphSnapshot,
    GRAPH_EDGE_PAGE_LIMIT,
};
#[cfg(feature = "read-write")]
pub use graphs::{
    persist_dependency_graph, DependencyGraphArtifactInput, DependencyGraphEdgeInput,
    DependencyGraphPersistReceipt,
};

#[cfg(feature = "read-write")]
pub use search::upsert_embedding;
pub use search::{search_registry, semantic_search, EmbeddingInput, RegistrySearchHit};

#[cfg(feature = "read-write")]
mod artifacts;
#[cfg(feature = "read-write")]
mod licenses;

#[cfg(feature = "read-write")]
pub use artifacts::{
    record_package_download, register_package_upload, PackageDownloadInput, PackageUploadInput,
};
#[cfg(feature = "read-write")]
pub use licenses::{add_package_license, PackageLicenseInput};
