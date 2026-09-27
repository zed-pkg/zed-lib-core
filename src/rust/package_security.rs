//! Deterministic verification for the portable package dependency closure contract.
//!
//! Shape and structural validation live in `zed-interfaces`. This module owns
//! behavior: canonical dependency ordering, RFC 8785-compatible serialization
//! for the v1 all-string closure shape, duplicate/root substitution rejection,
//! and recomputation of the closure SHA-256.
//!
//! A verified closure is evidence only. It is not a package approval or a
//! deployment authorization.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct IdentityKey<'a> {
    ecosystem: &'a str,
    registry_uri: &'a str,
    package_name: &'a str,
    package_version: &'a str,
    resolved_revision: &'a str,
    artifact_digest: &'a str,
    source_digest: &'a str,
    outer_checksum: &'a str,
}

pub fn verify_package_dependency_closure(
    closure: &PackageDependencyClosure,
) -> Result<VerifiedPackageDependencyClosure, PackageClosureError> {
    closure.validate()?;

    let root_key = identity_key(&closure.root);
    let mut dependencies = closure.dependencies.clone();
    dependencies.sort_by(|left, right| identity_key(left).cmp(&identity_key(right)));

    let mut previous: Option<IdentityKey<'_>> = None;
    for dependency in &dependencies {
        let key = identity_key(dependency);
        if key == root_key {
            return Err(PackageClosureError::RootRepeated);
        }
        if previous.as_ref().is_some_and(|prior| *prior == key) {
            return Err(PackageClosureError::DuplicateDependency);
        }
        previous = Some(key);
    }

    let canonical_json = canonical_closure_json(closure, &dependencies)?;
    let actual = format!("{:x}", Sha256::digest(canonical_json.as_bytes()));
    if actual != closure.closure_digest {
        return Err(PackageClosureError::DigestMismatch {
            expected: closure.closure_digest.clone(),
            actual,
        });
    }

    let mut normalized = closure.clone();
    normalized.dependencies = dependencies;

    Ok(VerifiedPackageDependencyClosure {
        closure: normalized,
        canonical_json,
    })
}

fn identity_key(identity: &PackageArtifactIdentity) -> IdentityKey<'_> {
    match identity {
        PackageArtifactIdentity::Registry(identity) => IdentityKey {
            ecosystem: registry_ecosystem(identity.ecosystem),
            registry_uri: "",
            package_name: &identity.package_name,
            package_version: &identity.package_version,
            resolved_revision: &identity.resolved_revision,
            artifact_digest: &identity.artifact_digest,
            source_digest: &identity.source_digest,
            outer_checksum: "",
        },
        PackageArtifactIdentity::Hex(identity) => IdentityKey {
            ecosystem: "hex",
            registry_uri: &identity.registry_uri,
            package_name: &identity.package_name,
            package_version: &identity.package_version,
            resolved_revision: &identity.resolved_revision,
            artifact_digest: &identity.artifact_digest,
            source_digest: &identity.source_digest,
            outer_checksum: &identity.outer_checksum,
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
    use zed_interfaces::package_security::{HexPackageEcosystem, PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1};

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
    fn verifies_known_rfc8785_fixture_and_normalizes_dependency_order() {
        let verified =
            verify_package_dependency_closure(&fixture_closure()).expect("fixture verifies");
        assert_eq!(verified.closure_digest(), EXPECTED_FIXTURE_DIGEST);
        assert_eq!(
            identity_key(&verified.closure().dependencies[0]).package_name,
            "gleam_json"
        );
        assert_eq!(
            identity_key(&verified.closure().dependencies[1]).package_name,
            "gleam_stdlib"
        );
        assert!(!verified.canonical_json().contains("closure_digest"));
    }

    #[test]
    fn dependency_input_order_does_not_change_digest() {
        let mut closure = fixture_closure();
        closure.dependencies.reverse();
        verify_package_dependency_closure(&closure).expect("reordered fixture verifies");
    }

    #[test]
    fn rejects_duplicate_dependency_identity_even_if_other_fields_match() {
        let mut closure = fixture_closure();
        closure.dependencies.push(closure.dependencies[0].clone());
        assert_eq!(
            verify_package_dependency_closure(&closure)
                .expect_err("duplicate must fail")
                .kind(),
            "duplicate_dependency"
        );
    }

    #[test]
    fn rejects_root_substitution_into_dependency_set() {
        let mut closure = fixture_closure();
        closure.dependencies.push(closure.root.clone());
        assert_eq!(
            verify_package_dependency_closure(&closure)
                .expect_err("root repetition must fail")
                .kind(),
            "root_repeated"
        );
    }

    #[test]
    fn rejects_supplied_digest_instead_of_trusting_it() {
        let mut closure = fixture_closure();
        closure.closure_digest = digest('9');
        let error =
            verify_package_dependency_closure(&closure).expect_err("forged digest must fail");
        assert_eq!(error.kind(), "digest_mismatch");
        match error {
            PackageClosureError::DigestMismatch { actual, .. } => {
                assert_eq!(actual, EXPECTED_FIXTURE_DIGEST);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn changed_transitive_identity_invalidates_the_closure() {
        let mut closure = fixture_closure();
        let PackageArtifactIdentity::Hex(dependency) = &mut closure.dependencies[0] else {
            panic!("fixture dependency is Hex");
        };
        dependency.outer_checksum = digest('0');
        assert_eq!(
            verify_package_dependency_closure(&closure)
                .expect_err("changed bytes must invalidate digest")
                .kind(),
            "digest_mismatch"
        );
    }
}
