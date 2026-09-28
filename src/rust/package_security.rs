//! Semantic verification for the portable package-security dependency closure.
//!
//! Shape admission remains owned by the independent TypeSpec and Draft 2020-12
//! JSON Schema authorities in `zed-interfaces/validation/package-security`.
//! This module accepts an already-admitted JSON value rather than declaring a
//! competing serde DTO. It owns only behavior the schemas cannot express:
//! normalization, deterministic ordering, duplicate/root rejection, canonical
//! hashing, and verification of `closure_digest`.

use std::cmp::Ordering;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1: &str = "zed-package-dependency-closure-v1";
pub const PACKAGE_DEPENDENCY_CLOSURE_MAX_DEPENDENCIES: usize = 4096;

const TOP_LEVEL_FIELDS: &[&str] = &[
    "closure_digest",
    "dependencies",
    "dependency_lock_digest",
    "format",
    "resolver_id",
    "resolver_version",
    "root",
];
const REGISTRY_IDENTITY_FIELDS: &[&str] = &[
    "artifact_digest",
    "ecosystem",
    "package_name",
    "package_version",
    "resolved_revision",
    "source_digest",
    "source_uri",
];
const HEX_IDENTITY_FIELDS: &[&str] = &[
    "artifact_digest",
    "ecosystem",
    "outer_checksum",
    "package_name",
    "package_version",
    "registry_uri",
    "resolved_revision",
    "source_digest",
    "source_uri",
];

#[derive(Debug, Clone, PartialEq)]
pub struct PackageDependencyClosureEvidence {
    normalized: Value,
    closure_digest: String,
}

impl PackageDependencyClosureEvidence {
    #[must_use]
    pub fn normalized(&self) -> &Value {
        &self.normalized
    }

    #[must_use]
    pub fn closure_digest(&self) -> &str {
        &self.closure_digest
    }

    #[must_use]
    pub fn into_normalized(self) -> Value {
        self.normalized
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageDependencyClosureError {
    #[error("dependency closure must be a JSON object admitted by package-security")]
    NotObject,
    #[error("dependency closure contains an unsupported or missing top-level field")]
    InvalidTopLevelShape,
    #[error("dependency closure format is not zed-package-dependency-closure-v1")]
    InvalidFormat,
    #[error("dependency closure field {field} must be a non-empty string")]
    InvalidStringField { field: String },
    #[error("digest field {field} must be exactly 64 hexadecimal characters")]
    InvalidDigest { field: String },
    #[error("dependency closure dependencies must be an array")]
    DependenciesNotArray,
    #[error("dependency closure has too many dependencies: {actual}")]
    TooManyDependencies { actual: usize },
    #[error("package artifact identity at {location} is not an admitted v1 identity")]
    InvalidIdentityShape { location: String },
    #[error("unsupported package ecosystem {ecosystem:?} at {location}")]
    UnsupportedEcosystem { ecosystem: String, location: String },
    #[error("dependency closure repeats dependency identity {identity}")]
    DuplicateDependency { identity: String },
    #[error("dependency closure repeats the root artifact inside dependencies")]
    RootRepeated,
    #[error("canonical closure contains a non-string semantic value at {location}")]
    UnsupportedCanonicalValue { location: String },
    #[error("could not encode canonical JSON: {message}")]
    CanonicalJson { message: String },
    #[error("closure_digest mismatch: supplied {supplied}, recomputed {recomputed}")]
    DigestMismatch {
        supplied: String,
        recomputed: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentitySortKey {
    ecosystem: String,
    registry_uri: String,
    package_name: String,
    package_version: String,
    resolved_revision: String,
    artifact_digest: String,
    source_digest: String,
    outer_checksum: String,
}

impl IdentitySortKey {
    fn display_identity(&self) -> String {
        format!(
            "{}:{}@{}:{}",
            self.ecosystem, self.package_name, self.package_version, self.artifact_digest
        )
    }
}

/// Recompute the v1 digest and return normalized evidence.
///
/// The input must first pass the package-security TJSV peer-authority gate.
/// This function does not replace schema admission.
pub fn recompute_package_dependency_closure(
    value: &Value,
) -> Result<PackageDependencyClosureEvidence, PackageDependencyClosureError> {
    let (mut normalized, _) = normalize_semantics(value)?;
    let digest = compute_normalized_digest(&normalized)?;
    normalized
        .as_object_mut()
        .ok_or(PackageDependencyClosureError::NotObject)?
        .insert("closure_digest".to_owned(), Value::String(digest.clone()));

    Ok(PackageDependencyClosureEvidence {
        normalized,
        closure_digest: digest,
    })
}

/// Verify the supplied v1 `closure_digest` after semantic normalization.
pub fn verify_package_dependency_closure(
    value: &Value,
) -> Result<PackageDependencyClosureEvidence, PackageDependencyClosureError> {
    let (mut normalized, supplied) = normalize_semantics(value)?;
    let recomputed = compute_normalized_digest(&normalized)?;
    if supplied != recomputed {
        return Err(PackageDependencyClosureError::DigestMismatch {
            supplied,
            recomputed,
        });
    }

    normalized
        .as_object_mut()
        .ok_or(PackageDependencyClosureError::NotObject)?
        .insert(
            "closure_digest".to_owned(),
            Value::String(recomputed.clone()),
        );

    Ok(PackageDependencyClosureEvidence {
        normalized,
        closure_digest: recomputed,
    })
}

fn normalize_semantics(
    value: &Value,
) -> Result<(Value, String), PackageDependencyClosureError> {
    let mut normalized = value.clone();
    let object = normalized
        .as_object_mut()
        .ok_or(PackageDependencyClosureError::NotObject)?;
    if !has_exact_fields(object, TOP_LEVEL_FIELDS) {
        return Err(PackageDependencyClosureError::InvalidTopLevelShape);
    }

    if string_field(object, "format", "format")? != PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1 {
        return Err(PackageDependencyClosureError::InvalidFormat);
    }

    let lock_digest =
        normalize_digest_field(object, "dependency_lock_digest", "dependency_lock_digest")?;
    object.insert(
        "dependency_lock_digest".to_owned(),
        Value::String(lock_digest),
    );

    let supplied = normalize_digest_field(object, "closure_digest", "closure_digest")?;
    object.insert(
        "closure_digest".to_owned(),
        Value::String(supplied.clone()),
    );

    for field in ["resolver_id", "resolver_version"] {
        if string_field(object, field, field)?.is_empty() {
            return Err(PackageDependencyClosureError::InvalidStringField {
                field: field.to_owned(),
            });
        }
    }

    let root_key = {
        let root = object
            .get_mut("root")
            .ok_or(PackageDependencyClosureError::InvalidTopLevelShape)?;
        normalize_identity(root, "root")?
    };

    let dependencies = object
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or(PackageDependencyClosureError::DependenciesNotArray)?;
    if dependencies.len() > PACKAGE_DEPENDENCY_CLOSURE_MAX_DEPENDENCIES {
        return Err(PackageDependencyClosureError::TooManyDependencies {
            actual: dependencies.len(),
        });
    }

    let original = std::mem::take(dependencies);
    let mut normalized_dependencies = Vec::with_capacity(original.len());
    for (index, mut dependency) in original.into_iter().enumerate() {
        let location = format!("dependencies[{index}]");
        let key = normalize_identity(&mut dependency, &location)?;
        normalized_dependencies.push((key, dependency));
    }

    normalized_dependencies.sort_by(|left, right| compare_identity_keys(&left.0, &right.0));
    for pair in normalized_dependencies.windows(2) {
        if compare_identity_keys(&pair[0].0, &pair[1].0) == Ordering::Equal {
            return Err(PackageDependencyClosureError::DuplicateDependency {
                identity: pair[0].0.display_identity(),
            });
        }
    }
    if normalized_dependencies
        .iter()
        .any(|(key, _)| compare_identity_keys(key, &root_key) == Ordering::Equal)
    {
        return Err(PackageDependencyClosureError::RootRepeated);
    }

    dependencies.extend(
        normalized_dependencies
            .into_iter()
            .map(|(_, dependency)| dependency),
    );
    Ok((normalized, supplied))
}

fn normalize_identity(
    value: &mut Value,
    location: &str,
) -> Result<IdentitySortKey, PackageDependencyClosureError> {
    let object = value.as_object_mut().ok_or_else(|| {
        PackageDependencyClosureError::InvalidIdentityShape {
            location: location.to_owned(),
        }
    })?;
    let ecosystem = string_field(object, "ecosystem", &format!("{location}.ecosystem"))?;

    match ecosystem.as_str() {
        "hex" if has_exact_fields(object, HEX_IDENTITY_FIELDS) => {}
        "npm" | "cargo" | "python" | "git"
            if has_exact_fields(object, REGISTRY_IDENTITY_FIELDS) => {}
        "hex" | "npm" | "cargo" | "python" | "git" => {
            return Err(PackageDependencyClosureError::InvalidIdentityShape {
                location: location.to_owned(),
            });
        }
        _ => {
            return Err(PackageDependencyClosureError::UnsupportedEcosystem {
                ecosystem,
                location: location.to_owned(),
            });
        }
    }

    let package_name = non_empty_string(object, "package_name", location)?;
    let package_version = non_empty_string(object, "package_version", location)?;
    let _source_uri = non_empty_string(object, "source_uri", location)?;
    let resolved_revision = non_empty_string(object, "resolved_revision", location)?;

    let source_digest =
        normalize_digest_field(object, "source_digest", &format!("{location}.source_digest"))?;
    object.insert(
        "source_digest".to_owned(),
        Value::String(source_digest.clone()),
    );
    let artifact_digest = normalize_digest_field(
        object,
        "artifact_digest",
        &format!("{location}.artifact_digest"),
    )?;
    object.insert(
        "artifact_digest".to_owned(),
        Value::String(artifact_digest.clone()),
    );

    let (registry_uri, outer_checksum) = if ecosystem == "hex" {
        let registry_uri = non_empty_string(object, "registry_uri", location)?;
        let outer_checksum = normalize_digest_field(
            object,
            "outer_checksum",
            &format!("{location}.outer_checksum"),
        )?;
        object.insert(
            "outer_checksum".to_owned(),
            Value::String(outer_checksum.clone()),
        );
        (registry_uri, outer_checksum)
    } else {
        (String::new(), String::new())
    };

    Ok(IdentitySortKey {
        ecosystem,
        registry_uri,
        package_name,
        package_version,
        resolved_revision,
        artifact_digest,
        source_digest,
        outer_checksum,
    })
}

fn has_exact_fields(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    object.len() == allowed.len()
        && object
            .keys()
            .all(|field| allowed.contains(&field.as_str()))
}

fn string_field(
    object: &Map<String, Value>,
    field: &str,
    error_field: &str,
) -> Result<String, PackageDependencyClosureError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| PackageDependencyClosureError::InvalidStringField {
            field: error_field.to_owned(),
        })
}

fn non_empty_string(
    object: &Map<String, Value>,
    field: &str,
    location: &str,
) -> Result<String, PackageDependencyClosureError> {
    let value = string_field(object, field, &format!("{location}.{field}"))?;
    if value.is_empty() {
        return Err(PackageDependencyClosureError::InvalidStringField {
            field: format!("{location}.{field}"),
        });
    }
    Ok(value)
}

fn normalize_digest_field(
    object: &Map<String, Value>,
    field: &str,
    error_field: &str,
) -> Result<String, PackageDependencyClosureError> {
    let raw = string_field(object, field, error_field)?;
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PackageDependencyClosureError::InvalidDigest {
            field: error_field.to_owned(),
        });
    }
    Ok(raw.to_ascii_lowercase())
}

fn compare_identity_keys(left: &IdentitySortKey, right: &IdentitySortKey) -> Ordering {
    for ordering in [
        compare_utf16(&left.ecosystem, &right.ecosystem),
        compare_utf16(&left.registry_uri, &right.registry_uri),
        compare_utf16(&left.package_name, &right.package_name),
        compare_utf16(&left.package_version, &right.package_version),
        compare_utf16(&left.resolved_revision, &right.resolved_revision),
        compare_utf16(&left.artifact_digest, &right.artifact_digest),
        compare_utf16(&left.source_digest, &right.source_digest),
        compare_utf16(&left.outer_checksum, &right.outer_checksum),
    ] {
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_utf16(left: &str, right: &str) -> Ordering {
    let mut left_units = left.encode_utf16();
    let mut right_units = right.encode_utf16();
    loop {
        match (left_units.next(), right_units.next()) {
            (Some(left_unit), Some(right_unit)) => match left_unit.cmp(&right_unit) {
                Ordering::Equal => {}
                ordering => return ordering,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// RFC 8785 JCS is broader than this contract, but the admitted closure domain
/// contains only objects, arrays, and strings. Canonicalizing exactly that
/// restricted domain avoids depending on a looser general-purpose serializer.
fn compute_normalized_digest(
    normalized: &Value,
) -> Result<String, PackageDependencyClosureError> {
    let mut preimage = normalized.clone();
    preimage
        .as_object_mut()
        .ok_or(PackageDependencyClosureError::NotObject)?
        .remove("closure_digest");

    let mut canonical = String::new();
    write_canonical_json(&preimage, "$", &mut canonical)?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn write_canonical_json(
    value: &Value,
    location: &str,
    output: &mut String,
) -> Result<(), PackageDependencyClosureError> {
    match value {
        Value::String(text) => {
            output.push_str(&serde_json::to_string(text).map_err(|error| {
                PackageDependencyClosureError::CanonicalJson {
                    message: error.to_string(),
                }
            })?);
        }
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_json(item, &format!("{location}[{index}]"), output)?;
            }
            output.push(']');
        }
        Value::Object(object) => {
            output.push('{');
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| compare_utf16(left.0, right.0));
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).map_err(|error| {
                    PackageDependencyClosureError::CanonicalJson {
                        message: error.to_string(),
                    }
                })?);
                output.push(':');
                write_canonical_json(item, &format!("{location}.{key}"), output)?;
            }
            output.push('}');
        }
        _ => {
            return Err(PackageDependencyClosureError::UnsupportedCanonicalValue {
                location: location.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity(name: &str, artifact: char, source: char) -> Value {
        json!({
            "ecosystem": "hex",
            "package_name": name,
            "package_version": "1.0.0",
            "source_uri": format!("https://github.com/example/{name}"),
            "source_digest": source.to_string().repeat(64),
            "resolved_revision": "1.0.0",
            "artifact_digest": artifact.to_string().repeat(64),
            "registry_uri": "https://repo.hex.pm",
            "outer_checksum": "c".repeat(64)
        })
    }

    fn closure() -> Value {
        json!({
            "format": PACKAGE_DEPENDENCY_CLOSURE_FORMAT_V1,
            "root": identity("root", 'b', 'a'),
            "dependencies": [
                identity("zed", 'e', 'd'),
                identity("alpha", '2', '1')
            ],
            "dependency_lock_digest": "4".repeat(64),
            "closure_digest": "0".repeat(64),
            "resolver_id": "zed-hex-resolver",
            "resolver_version": "1.0.0"
        })
    }

    fn with_computed_digest(value: &Value) -> Value {
        recompute_package_dependency_closure(value)
            .expect("fixture recomputes")
            .into_normalized()
    }

    #[test]
    fn dependency_input_order_does_not_change_digest() {
        let first = closure();
        let mut second = closure();
        second
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("dependencies")
            .reverse();

        let first = recompute_package_dependency_closure(&first).expect("first");
        let second = recompute_package_dependency_closure(&second).expect("second");
        assert_eq!(first.closure_digest(), second.closure_digest());
        assert_eq!(first.normalized(), second.normalized());
    }

    #[test]
    fn hexadecimal_evidence_is_normalized_before_hashing() {
        let mut value = closure();
        value["root"]["artifact_digest"] = Value::String("AB".repeat(32));
        value["dependency_lock_digest"] = Value::String("CD".repeat(32));

        let evidence = recompute_package_dependency_closure(&value).expect("recomputes");
        assert_eq!(
            evidence.normalized()["root"]["artifact_digest"],
            Value::String("ab".repeat(32))
        );
        assert_eq!(
            evidence.normalized()["dependency_lock_digest"],
            Value::String("cd".repeat(32))
        );
    }

    #[test]
    fn forged_or_stale_digest_is_rejected() {
        let value = closure();
        assert!(matches!(
            verify_package_dependency_closure(&value),
            Err(PackageDependencyClosureError::DigestMismatch { .. })
        ));
        assert!(verify_package_dependency_closure(&with_computed_digest(&value)).is_ok());
    }

    #[test]
    fn duplicate_dependency_identity_is_rejected() {
        let mut value = closure();
        let duplicate = value["dependencies"][0].clone();
        value
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("dependencies")
            .push(duplicate);

        assert!(matches!(
            recompute_package_dependency_closure(&value),
            Err(PackageDependencyClosureError::DuplicateDependency { .. })
        ));
    }

    #[test]
    fn root_repeated_in_dependencies_is_rejected() {
        let mut value = closure();
        let root = value["root"].clone();
        value
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("dependencies")
            .push(root);

        assert_eq!(
            recompute_package_dependency_closure(&value).unwrap_err(),
            PackageDependencyClosureError::RootRepeated
        );
    }

    #[test]
    fn exact_lock_and_resolver_identity_are_digest_bound() {
        let base = recompute_package_dependency_closure(&closure())
            .expect("base")
            .closure_digest()
            .to_owned();

        for (field, replacement) in [
            ("dependency_lock_digest", "9".repeat(64)),
            ("resolver_id", "other-resolver".to_owned()),
            ("resolver_version", "2.0.0".to_owned()),
        ] {
            let mut changed = closure();
            changed[field] = Value::String(replacement);
            let digest = recompute_package_dependency_closure(&changed)
                .expect("changed")
                .closure_digest()
                .to_owned();
            assert_ne!(base, digest, "{field} must be digest-bound");
        }
    }

    #[test]
    fn artifact_source_and_outer_checksum_each_change_identity() {
        let base = recompute_package_dependency_closure(&closure())
            .expect("base")
            .closure_digest()
            .to_owned();

        for field in ["artifact_digest", "source_digest", "outer_checksum"] {
            let mut changed = closure();
            changed["dependencies"][0][field] = Value::String("9".repeat(64));
            let digest = recompute_package_dependency_closure(&changed)
                .expect("changed")
                .closure_digest()
                .to_owned();
            assert_ne!(base, digest, "{field} must be digest-bound");
        }
    }

    #[test]
    fn dependency_membership_changes_closure_identity() {
        let base = recompute_package_dependency_closure(&closure())
            .expect("base")
            .closure_digest()
            .to_owned();
        let mut changed = closure();
        changed
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("dependencies")
            .pop();

        let digest = recompute_package_dependency_closure(&changed)
            .expect("changed")
            .closure_digest()
            .to_owned();
        assert_ne!(base, digest);
    }

    #[test]
    fn dependency_cap_fails_closed() {
        let mut value = closure();
        let dependency = identity("member", '8', '7');
        let dependencies = value
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("dependencies");
        dependencies.clear();
        for index in 0..=PACKAGE_DEPENDENCY_CLOSURE_MAX_DEPENDENCIES {
            let mut member = dependency.clone();
            member["package_name"] = Value::String(format!("member-{index:04}"));
            dependencies.push(member);
        }

        assert_eq!(
            recompute_package_dependency_closure(&value).unwrap_err(),
            PackageDependencyClosureError::TooManyDependencies {
                actual: PACKAGE_DEPENDENCY_CLOSURE_MAX_DEPENDENCIES + 1
            }
        );
    }

    #[test]
    fn unexpected_identity_fields_fail_closed() {
        let mut value = closure();
        value["root"]["unexpected"] = Value::String("not-authority".to_owned());

        assert!(matches!(
            recompute_package_dependency_closure(&value),
            Err(PackageDependencyClosureError::InvalidIdentityShape { .. })
        ));
    }
}
