//! Deterministic verification for the portable package dependency closure contract.
//!
//! Shape and structural validation live in `zed-interfaces`. This module owns
//! behavior: canonical dependency ordering, RFC 8785-compatible serialization
//! for the v1 all-string closure shape, duplicate/root substitution rejection,
//! and recomputation of the closure SHA-256.
//!
//! A verified closure is evidence only. It is not a package approval or a
//! deployment authorization.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zed_interfaces::package_security::{
    HexPackageArtifactIdentity, PackageArtifactIdentity, PackageDependencyClosure,
    PackageSecurityContractError, RegistryPackageArtifactIdentity, RegistryPackageEcosystem,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPackageDependencyClosure {
    closure: PackageDependencyClosure,
    canonical_json: String,
}

impl VerifiedPackageDependencyClosure {
    #[must_use]
    pub fn closure(&self) -> &PackageDependencyClosure {
        &self.closure
    }

    #[must_use]
    pub fn closure_digest(&self) -> &str {
        &self.closure.closure_digest
    }

    #[must_use]
    pub fn canonical_json(&self) -> &str {
        &self.canonical_json
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageClosureError {
    #[error("invalid package dependency closure: {0}")]
    InvalidContract(#[from] PackageSecurityContractError),
    #[error("package dependency closure contains a duplicate dependency identity")]
    DuplicateDependency,
    #[error("package dependency closure repeats its root artifact as a dependency")]
    RootRepeated,
    #[error("package dependency closure digest mismatch: expected {expected}, recomputed {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("package dependency closure canonical JSON serialization failed: {0}")]
    CanonicalJson(String),
}

impl PackageClosureError {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::InvalidContract(_) => "invalid_contract",
            Self::DuplicateDependency => "duplicate_dependency",
            Self::RootRepeated => "root_repeated",
            Self::DigestMismatch { .. } => "digest_mismatch",
            Self::CanonicalJson(_) => "canonical_json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IdentityKey {
    ecosystem: String,
    registry_uri: String,
    package_name: String,
    package_version: String,
    resolved_revision: String,
    artifact_digest: String,
    source_digest: String,
    outer_checksum: String,
}

pub fn verify_package_dependency_closure(
    closure: &PackageDependencyClosure,
) -> Result<VerifiedPackageDependencyClosure, PackageClosureError> {
    closure.validate()?;

    let root_key = identity_key(&closure.root);
    let keyed_dependencies = closure
        .dependencies
        .iter()
        .cloned()
        .map(|dependency| (identity_key(&dependency), dependency))
        .collect::<BTreeMap<_, _>>();

    if keyed_dependencies.len() != closure.dependencies.len() {
        return Err(PackageClosureError::DuplicateDependency);
    }
    if keyed_dependencies.contains_key(&root_key) {
        return Err(PackageClosureError::RootRepeated);
    }

    let dependencies = keyed_dependencies.into_values().collect::<Vec<_>>();
    let canonical_json = canonical_closure_json(closure, &dependencies)?;
    let actual = format!("{:x}", Sha256::digest(canonical_json.as_bytes()));
    if actual != closure.closure_digest {
        return Err(PackageClosureError::DigestMismatch {
            expected: closure.closure_digest.clone(),
            actual,
        });
    }

    let normalized = PackageDependencyClosure {
        dependencies,
        ..closure.clone()
    };

    Ok(VerifiedPackageDependencyClosure {
        closure: normalized,
        canonical_json,
    })
}

fn identity_key(identity: &PackageArtifactIdentity) -> IdentityKey {
    match identity {
        PackageArtifactIdentity::Registry(identity) => IdentityKey {
            ecosystem: registry_ecosystem(identity.ecosystem).into(),
            registry_uri: String::new(),
            package_name: identity.package_name.clone(),
            package_version: identity.package_version.clone(),
            resolved_revision: identity.resolved_revision.clone(),
            artifact_digest: identity.artifact_digest.clone(),
            source_digest: identity.source_digest.clone(),
            outer_checksum: String::new(),
        },
        PackageArtifactIdentity::Hex(identity) => IdentityKey {
            ecosystem: "hex".into(),
            registry_uri: identity.registry_uri.clone(),
            package_name: identity.package_name.clone(),
            package_version: identity.package_version.clone(),
            resolved_revision: identity.resolved_revision.clone(),
            artifact_digest: identity.artifact_digest.clone(),
            source_digest: identity.source_digest.clone(),
            outer_checksum: identity.outer_checksum.clone(),
        },
    }
}

const fn registry_ecosystem(ecosystem: RegistryPackageEcosystem) -> &'static str {
    match ecosystem {
        RegistryPackageEcosystem::Npm => "npm",
        RegistryPackageEcosystem::Cargo => "cargo",
        RegistryPackageEcosystem::Python => "python",
        RegistryPackageEcosystem::Git => "git",
    }
}

fn canonical_closure_json(
    closure: &PackageDependencyClosure,
    dependencies: &[PackageArtifactIdentity],
) -> Result<String, PackageClosureError> {
    let dependency_json = dependencies
        .iter()
        .map(canonical_identity_json)
        .collect::<Result<Vec<_>, _>>()?
        .join(",");

    Ok(format!(
        "{{\"dependencies\":[{dependency_json}],\"dependency_lock_digest\":{},\"format\":{},\"resolver_id\":{},\"resolver_version\":{},\"root\":{}}}",
        json_string(&closure.dependency_lock_digest)?,
        json_string(&closure.format)?,
        json_string(&closure.resolver_id)?,
        json_string(&closure.resolver_version)?,
        canonical_identity_json(&closure.root)?,
    ))
}

fn canonical_identity_json(identity: &PackageArtifactIdentity) -> Result<String, PackageClosureError> {
    match identity {
        PackageArtifactIdentity::Registry(identity) => canonical_registry_identity_json(identity),
        PackageArtifactIdentity::Hex(identity) => canonical_hex_identity_json(identity),
    }
}

fn canonical_registry_identity_json(
    identity: &RegistryPackageArtifactIdentity,
) -> Result<String, PackageClosureError> {
    Ok(format!(
        "{{\"artifact_digest\":{},\"ecosystem\":{},\"package_name\":{},\"package_version\":{},\"resolved_revision\":{},\"source_digest\":{},\"source_uri\":{}}}",
        json_string(&identity.artifact_digest)?,
        json_string(registry_ecosystem(identity.ecosystem))?,
        json_string(&identity.package_name)?,
        json_string(&identity.package_version)?,
        json_string(&identity.resolved_revision)?,
        json_string(&identity.source_digest)?,
        json_string(&identity.source_uri)?,
    ))
}

fn canonical_hex_identity_json(
    identity: &HexPackageArtifactIdentity,
) -> Result<String, PackageClosureError> {
    Ok(format!(
        "{{\"artifact_digest\":{},\"ecosystem\":\"hex\",\"outer_checksum\":{},\"package_name\":{},\"package_version\":{},\"registry_uri\":{},\"resolved_revision\":{},\"source_digest\":{},\"source_uri\":{}}}",
        json_string(&identity.artifact_digest)?,
        json_string(&identity.outer_checksum)?,
        json_string(&identity.package_name)?,
        json_string(&identity.package_version)?,
        json_string(&identity.registry_uri)?,
        json_string(&identity.resolved_revision)?,
        json_string(&identity.source_digest)?,
        json_string(&identity.source_uri)?,
    ))
}

fn json_string(value: &str) -> Result<String, PackageClosureError> {
    serde_json::to_string(value).map_err(|error| PackageClosureError::CanonicalJson(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zed_interfaces::package_security::{
        HexPackageEcosystem, PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1,
    };

    const EXPECTED_FIXTURE_DIGEST: &str =
        "2f704cfa7c9f59894d97d4b2552e7dec0e6bdee6852a70b1dca8cd50b9e78684";

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn hex_identity(
        name: &str,
        version: &str,
        source_digest: char,
        artifact_digest: char,
        outer_checksum: char,
    ) -> PackageArtifactIdentity {
        PackageArtifactIdentity::Hex(HexPackageArtifactIdentity {
            ecosystem: HexPackageEcosystem::Hex,
            package_name: name.into(),
            package_version: version.into(),
            source_uri: format!("https://github.com/example/{name}"),
            source_digest: digest(source_digest),
            resolved_revision: version.into(),
            artifact_digest: digest(artifact_digest),
            registry_uri: "https://repo.hex.pm".into(),
            outer_checksum: digest(outer_checksum),
        })
    }

    fn fixture_closure() -> PackageDependencyClosure {
        PackageDependencyClosure {
            format: PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1.into(),
            root: hex_identity("beam_app", "1.0.0", 'a', 'b', 'c'),
            dependencies: vec![
                hex_identity("gleam_stdlib", "0.62.1", 'd', 'e', 'f'),
                hex_identity("gleam_json", "3.0.2", '1', '2', '3'),
            ],
            dependency_lock_digest: digest('4'),
            closure_digest: EXPECTED_FIXTURE_DIGEST.into(),
            resolver_id: "zed-hex-resolver".into(),
            resolver_version: "1.0.0".into(),
        }
    }

    #[test]
    fn verifies_known_rfc8785_fixture_and_normalizes_dependency_order() -> Result<(), String> {
        let verified =
            verify_package_dependency_closure(&fixture_closure()).map_err(|error| error.to_string())?;
        let names = verified
            .closure()
            .dependencies
            .iter()
            .map(identity_key)
            .map(|key| key.package_name)
            .collect::<Vec<_>>();

        assert_eq!(verified.closure_digest(), EXPECTED_FIXTURE_DIGEST);
        assert_eq!(names, vec!["gleam_json", "gleam_stdlib"]);
        assert!(!verified.canonical_json().contains("closure_digest"));
        Ok(())
    }

    #[test]
    fn dependency_input_order_does_not_change_digest() -> Result<(), String> {
        let original = fixture_closure();
        let closure = PackageDependencyClosure {
            dependencies: original.dependencies.iter().cloned().rev().collect(),
            ..original
        };
        verify_package_dependency_closure(&closure).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn rejects_duplicate_dependency_identity_even_if_other_fields_match() -> Result<(), String> {
        let original = fixture_closure();
        let duplicate = original
            .dependencies
            .first()
            .cloned()
            .ok_or_else(|| "fixture must have a dependency".to_string())?;
        let closure = PackageDependencyClosure {
            dependencies: original
                .dependencies
                .iter()
                .cloned()
                .chain(std::iter::once(duplicate))
                .collect(),
            ..original
        };
        let Err(error) = verify_package_dependency_closure(&closure) else {
            return Err("duplicate dependency unexpectedly verified".into());
        };
        assert_eq!(error.kind(), "duplicate_dependency");
        Ok(())
    }

    #[test]
    fn rejects_root_substitution_into_dependency_set() -> Result<(), String> {
        let original = fixture_closure();
        let root = original.root.clone();
        let closure = PackageDependencyClosure {
            dependencies: original
                .dependencies
                .iter()
                .cloned()
                .chain(std::iter::once(root))
                .collect(),
            ..original
        };
        let Err(error) = verify_package_dependency_closure(&closure) else {
            return Err("root repetition unexpectedly verified".into());
        };
        assert_eq!(error.kind(), "root_repeated");
        Ok(())
    }

    #[test]
    fn rejects_supplied_digest_instead_of_trusting_it() -> Result<(), String> {
        let closure = PackageDependencyClosure {
            closure_digest: digest('9'),
            ..fixture_closure()
        };
        let Err(error) = verify_package_dependency_closure(&closure) else {
            return Err("forged digest unexpectedly verified".into());
        };
        assert_eq!(error.kind(), "digest_mismatch");
        if let PackageClosureError::DigestMismatch { actual, .. } = error {
            assert_eq!(actual, EXPECTED_FIXTURE_DIGEST);
            return Ok(());
        }
        Err("digest mismatch returned the wrong error variant".into())
    }

    #[test]
    fn changed_transitive_identity_invalidates_the_closure() -> Result<(), String> {
        let original = fixture_closure();
        let dependencies = original
            .dependencies
            .iter()
            .cloned()
            .map(|dependency| match dependency {
                PackageArtifactIdentity::Hex(identity)
                    if identity.package_name == "gleam_stdlib" =>
                {
                    PackageArtifactIdentity::Hex(HexPackageArtifactIdentity {
                        outer_checksum: digest('0'),
                        ..identity
                    })
                }
                other => other,
            })
            .collect();
        let closure = PackageDependencyClosure {
            dependencies,
            ..original
        };

        let Err(error) = verify_package_dependency_closure(&closure) else {
            return Err("changed transitive identity unexpectedly verified".into());
        };
        assert_eq!(error.kind(), "digest_mismatch");
        Ok(())
    }
}
