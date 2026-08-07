//! Scheme-aware resolution against registry metadata.
//!
//! `zed-interfaces` answers "does this version string satisfy this
//! requirement?" without knowing which package is being asked about.
//! [`zed_interfaces::version::resolve`] takes a bare list of version strings,
//! so it cannot tell that a package declared itself `opaque` — and an opaque
//! package resolved through range algebra silently installs something its
//! publisher never promised.
//!
//! This module takes the whole [`PackageMetadata`] the registry served, so the
//! package's own [`VersionScheme`] decides how its requirement is read, and a
//! failure says which of the three different things went wrong instead of
//! collapsing to `None`.

use zed_interfaces::registry::PackageMetadata;
use zed_interfaces::version::{Requirement, VersionScheme, parse_version, resolve};

/// Why a requirement did not resolve. These are distinct on purpose: a typo in
/// a requirement and a package that simply has not published a matching
/// version need different fixes, and `Option::None` cannot tell them apart.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// The registry knows the package but it has no installable versions.
    #[error("{org}/{name} has no published versions")]
    NoVersions { org: String, name: String },

    /// The requirement cannot mean anything for this package's scheme.
    #[error("`{requirement}` is not a valid requirement for {org}/{name}: {reason}")]
    InvalidRequirement {
        org: String,
        name: String,
        requirement: String,
        reason: String,
    },

    /// A well-formed requirement that nothing published satisfies.
    #[error("{org}/{name} has no version matching `{requirement}`; published: {published}")]
    Unsatisfied {
        org: String,
        name: String,
        requirement: String,
        published: String,
    },
}

impl ResolveError {
    /// Stable machine-readable kind, shared with the conformance corpus so
    /// every language implementation agrees on *which* failure it is.
    pub fn kind(&self) -> &'static str {
        match self {
            ResolveError::NoVersions { .. } => "no_versions",
            ResolveError::InvalidRequirement { .. } => "invalid_requirement",
            ResolveError::Unsatisfied { .. } => "unsatisfied",
        }
    }
}

/// Resolve `requirement` against what the registry says a package published.
///
/// Returns the version in its **original spelling** — the store address and
/// the VCS tag have to stay faithful to what the publisher tagged, so a
/// normalized form is never substituted.
///
/// Under [`VersionScheme::Opaque`] a requirement must be an exact tag: those
/// packages have no ordering, so `^1.0` is not "everything 1.x", it is a
/// mistake that would otherwise resolve through semver algebra by accident.
pub fn resolve_version<'a>(
    metadata: &'a PackageMetadata,
    requirement: &str,
) -> Result<&'a str, ResolveError> {
    let (org, name) = (metadata.org.clone(), metadata.name.clone());

    if metadata.versions.is_empty() {
        return Err(ResolveError::NoVersions { org, name });
    }

    let scheme = metadata.version_scheme;
    let parsed = Requirement::parse(requirement);

    if scheme == VersionScheme::Opaque && matches!(parsed, Requirement::Range(_)) {
        return Err(ResolveError::InvalidRequirement {
            org,
            name,
            requirement: requirement.to_string(),
            reason: "opaque-versioned packages have no range algebra; require an exact tag"
                .to_string(),
        });
    }

    // A range that *looks* like one but does not parse (`^1.x.y`) would degrade
    // into an exact tag and never match. Catch it as the typo it is.
    if scheme != VersionScheme::Opaque
        && let Err(reason) = Requirement::validate(requirement)
    {
        return Err(ResolveError::InvalidRequirement {
            org,
            name,
            requirement: requirement.to_string(),
            reason,
        });
    }

    resolve(&parsed, &metadata.versions).ok_or_else(|| ResolveError::Unsatisfied {
        org,
        name,
        requirement: requirement.to_string(),
        published: metadata.versions.join(", "),
    })
}

/// The newest installable version, ignoring prereleases.
///
/// `PackageMetadata::latest` is what the registry computed when the package was
/// last published; it can lag a yank. This recomputes from the version list the
/// same response carried, so a client never offers a version the same payload
/// says is gone.
pub fn latest_stable(metadata: &PackageMetadata) -> Option<&str> {
    if metadata.version_scheme == VersionScheme::Opaque {
        // No ordering exists, so "newest" is only what the registry recorded —
        // and only if that version is still listed.
        return metadata
            .latest
            .as_deref()
            .filter(|latest| metadata.versions.iter().any(|v| v == latest));
    }
    metadata
        .versions
        .iter()
        .filter_map(|raw| parse_version(raw).map(|parsed| (parsed, raw)))
        .filter(|(parsed, _)| parsed.pre.is_empty())
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, raw)| raw.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zed_interfaces::vcs::Vcs;

    fn metadata(scheme: VersionScheme, versions: &[&str]) -> PackageMetadata {
        PackageMetadata {
            org: "acme".to_string(),
            name: "http-kit".to_string(),
            vcs: Vcs::Git,
            repo_url: "https://github.com/acme/http-kit".to_string(),
            description: None,
            latest: versions.last().map(|v| v.to_string()),
            versions: versions.iter().map(|v| v.to_string()).collect(),
            version_scheme: scheme,
            tags: Vec::new(),
        }
    }

    #[test]
    fn a_range_picks_the_highest_stable_match_in_its_published_spelling() {
        let meta = metadata(
            VersionScheme::Semver,
            &["1.0.0", "1.4.0", "1.5.0-rc.1", "2.0.0"],
        );
        assert_eq!(resolve_version(&meta, "^1.2").unwrap(), "1.4.0");
    }

    #[test]
    fn an_opaque_package_refuses_a_range_instead_of_resolving_it_by_accident() {
        let meta = metadata(
            VersionScheme::Opaque,
            &["legacy-api", "release-candidate-1"],
        );
        let error = resolve_version(&meta, "^1.0").unwrap_err();
        assert_eq!(error.kind(), "invalid_requirement");
        assert_eq!(resolve_version(&meta, "legacy-api").unwrap(), "legacy-api");
    }

    #[test]
    fn a_malformed_range_is_a_requirement_error_not_a_missing_version() {
        let meta = metadata(VersionScheme::Semver, &["1.0.0"]);
        assert_eq!(
            resolve_version(&meta, "^1.x.y").unwrap_err().kind(),
            "invalid_requirement"
        );
        assert_eq!(
            resolve_version(&meta, "^9.0").unwrap_err().kind(),
            "unsatisfied"
        );
    }

    #[test]
    fn a_package_with_nothing_published_says_so() {
        let meta = metadata(VersionScheme::Semver, &[]);
        assert_eq!(
            resolve_version(&meta, "^1.0").unwrap_err().kind(),
            "no_versions"
        );
    }

    #[test]
    fn calendar_versions_resolve_through_the_same_range_algebra() {
        let meta = metadata(VersionScheme::Calver, &["2026.06.01", "2026.07.24"]);
        assert_eq!(resolve_version(&meta, ">=2026.7").unwrap(), "2026.07.24");
    }

    #[test]
    fn latest_stable_ignores_prereleases_and_a_stale_latest_field() {
        let mut meta = metadata(VersionScheme::Semver, &["1.0.0", "2.0.0", "2.1.0-rc.1"]);
        meta.latest = Some("2.1.0-rc.1".to_string());
        assert_eq!(latest_stable(&meta), Some("2.0.0"));

        // A yanked `latest` that is no longer listed must not be offered.
        let mut opaque = metadata(VersionScheme::Opaque, &["legacy-api"]);
        opaque.latest = Some("withdrawn".to_string());
        assert_eq!(latest_stable(&opaque), None);
    }
}
