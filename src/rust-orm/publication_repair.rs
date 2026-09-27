#![allow(clippy::needless_return)]

//! Privileged compare-and-swap repair for one reviewed historical package
//! identity defect.
//!
//! Ordinary publication remains immutable in `publication`. This module is
//! deliberately separate so callers must opt into the exceptional repair
//! authority explicitly. Replacement bytes must already be staged and verified
//! at the canonical content-addressed key before this transaction is invoked.

use sea_orm::{
    prelude::Uuid, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait,
    QueryFilter, Statement, TransactionTrait, Value,
};

use crate::{
    entities::{audit_log, org, package, package_upload, package_version},
    OrmError, WriteContext,
};

const ARCHIVE_FORMATS: &[&str] = &["tar.gz", "tar.zst", "zip"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoricalDigestRepairClass {
    /// Zed CLI v0.3.0 fallback metadata could fingerprint raw GitHub source
    /// archives while publication used Zed's deterministic packer. The repair
    /// migrates only that historical split-brain into the deterministic bytes.
    LegacyGithubArchiveV030,
}

impl HistoricalDigestRepairClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyGithubArchiveV030 => {
                return "legacy-github-archive-v0.3.0";
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoricalDigestRepairInput {
    pub repair_class: HistoricalDigestRepairClass,
    pub org_slug: String,
    pub package_name: String,
    pub version: String,
    pub expected_old_sha256: String,
    pub expected_old_size_bytes: i64,
    pub expected_old_artifact_key: String,
    pub replacement_sha256: String,
    pub replacement_size_bytes: i64,
    pub replacement_artifact_key: String,
    pub format: String,
    pub vcs_tag: Option<String>,
    pub vcs_commit: Option<String>,
    /// Canonical Shared Auth admin subject. Stored in the append-only audit
    /// detail because admin identities intentionally do not map to customer
    /// registry user rows.
    pub actor_subject: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoricalDigestRepairReceipt {
    pub org_id: Uuid,
    pub package_id: Uuid,
    pub package_version_id: Uuid,
    pub old_sha256: String,
    pub replacement_sha256: String,
}

/// Compare-and-swap one known historical digest split-brain after replacement
/// bytes have already been staged under `replacement_artifact_key`.
///
/// This is not a republish or a general version override. Ordinary publication
/// remains immutable. The caller must arrive through the isolated admin repair
/// workflow and prove the exact previous immutable identity. Any drift between
/// the request and stored version aborts before mutation.
///
/// The canonical version row, every matching verified upload-ledger row, and
/// the append-only repair audit fact are changed in one database transaction.
///
/// # Errors
///
/// Returns a policy error for malformed repair evidence, a missing coordinate,
/// a mismatched old identity, a mismatched verified-upload ledger, or an
/// unsupported repair class. Database failures abort the transaction.
pub async fn repair_historical_machine_publish_digest(
    context: &WriteContext,
    input: HistoricalDigestRepairInput,
) -> Result<HistoricalDigestRepairReceipt, OrmError> {
    validate_historical_repair(&input)?;
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    lock_repair_coordinate(&transaction, &input).await?;
    let (organization, package, version) = load_repair_target(&transaction, &input).await?;
    let verified_uploads = load_verified_repair_uploads(&transaction, version.id, &input).await?;
    apply_repair_models(
        &transaction,
        &organization,
        &package,
        &version,
        verified_uploads,
        &input,
    )
    .await?;

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    return Ok(HistoricalDigestRepairReceipt {
        org_id: organization.id,
        package_id: package.id,
        package_version_id: version.id,
        old_sha256: input.expected_old_sha256,
        replacement_sha256: input.replacement_sha256,
    });
}

async fn lock_repair_coordinate<C: ConnectionTrait>(
    connection: &C,
    input: &HistoricalDigestRepairInput,
) -> Result<(), OrmError> {
    let lock_coordinate = format!(
        "zed-machine-publish:{}/{}",
        input.org_slug, input.package_name
    );
    connection
        .execute(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            [Value::String(Some(Box::new(lock_coordinate)))],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    return Ok(());
}

async fn load_repair_target<C: ConnectionTrait>(
    connection: &C,
    input: &HistoricalDigestRepairInput,
) -> Result<(org::Model, package::Model, package_version::Model), OrmError> {
    let organization = org::Entity::find()
        .filter(org::Column::Slug.eq(&input.org_slug))
        .filter(org::Column::IsSoftDeleted.eq(false))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| {
            return OrmError::not_found(format!(
                "repair organization {} does not exist",
                input.org_slug
            ));
        })?;
    let package = package::Entity::find()
        .filter(package::Column::OrgId.eq(organization.id))
        .filter(package::Column::Name.eq(&input.package_name))
        .filter(package::Column::IsSoftDeleted.eq(false))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| {
            return OrmError::not_found(format!(
                "repair package {}/{} does not exist",
                input.org_slug, input.package_name
            ));
        })?;
    let version = package_version::Entity::find()
        .filter(package_version::Column::PackageId.eq(package.id))
        .filter(package_version::Column::Version.eq(&input.version))
        .one(connection)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| {
            return OrmError::not_found(format!(
                "repair version {}/{}@{} does not exist",
                input.org_slug, input.package_name, input.version
            ));
        })?;
    if !repair_old_identity_matches(&version, input) {
        return Err(OrmError::policy(format!(
            "historical digest repair compare-and-swap failed for {}/{}@{}",
            input.org_slug, input.package_name, input.version
        )));
    }
    return Ok((organization, package, version));
}

async fn load_verified_repair_uploads<C: ConnectionTrait>(
    connection: &C,
    package_version_id: Uuid,
    input: &HistoricalDigestRepairInput,
) -> Result<Vec<package_upload::Model>, OrmError> {
    let uploads = package_upload::Entity::find()
        .filter(package_upload::Column::PackageVersionId.eq(package_version_id))
        .filter(package_upload::Column::Status.eq("verified"))
        .all(connection)
        .await
        .map_err(OrmError::from_db_err)?;
    if uploads.is_empty() {
        return Err(OrmError::policy(
            "historical digest repair requires an existing verified upload ledger",
        ));
    }
    if uploads.iter().any(|upload| {
        return upload.sha256.as_deref() != Some(input.expected_old_sha256.as_str())
            || upload.size_bytes != Some(input.expected_old_size_bytes)
            || upload.storage_key.as_deref() != Some(input.expected_old_artifact_key.as_str())
            || upload.format.as_deref() != Some(input.format.as_str());
    }) {
        return Err(OrmError::policy(
            "historical digest repair verified-upload ledger does not match expected old identity",
        ));
    }
    return Ok(uploads);
}

async fn apply_repair_models<C: ConnectionTrait>(
    connection: &C,
    organization: &org::Model,
    package: &package::Model,
    version: &package_version::Model,
    verified_uploads: Vec<package_upload::Model>,
    input: &HistoricalDigestRepairInput,
) -> Result<(), OrmError> {
    let replacement_version = package_version::ActiveModel {
        sha256: Set(input.replacement_sha256.clone()),
        size_bytes: Set(input.replacement_size_bytes),
        artifact_key: Set(input.replacement_artifact_key.clone()),
        ..version.clone().into()
    };
    replacement_version
        .update(connection)
        .await
        .map_err(OrmError::from_db_err)?;

    let now = chrono::Utc::now().fixed_offset();
    for upload in verified_uploads {
        let replacement_upload = package_upload::ActiveModel {
            sha256: Set(Some(input.replacement_sha256.clone())),
            size_bytes: Set(Some(input.replacement_size_bytes)),
            storage_key: Set(Some(input.replacement_artifact_key.clone())),
            updated_at: Set(now),
            ..upload.into()
        };
        replacement_upload
            .update(connection)
            .await
            .map_err(OrmError::from_db_err)?;
    }

    insert_repair_audit(connection, organization.id, package, version, input, now).await?;
    return Ok(());
}

async fn insert_repair_audit<C: ConnectionTrait>(
    connection: &C,
    org_id: Uuid,
    package: &package::Model,
    version: &package_version::Model,
    input: &HistoricalDigestRepairInput,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), OrmError> {
    audit_log::ActiveModel {
        id: Set(Uuid::new_v4()),
        org_id: Set(Some(org_id)),
        actor_user_id: Set(None),
        api_token_id: Set(None),
        action: Set("package.version.digest_repair".to_owned()),
        entity_type: Set("package_version".to_owned()),
        entity_id: Set(Some(version.id)),
        detail: Set(serde_json::json!({
            "repair_class": input.repair_class.as_str(),
            "package": package.name,
            "version": input.version,
            "old_sha256": input.expected_old_sha256,
            "replacement_sha256": input.replacement_sha256,
            "old_size_bytes": input.expected_old_size_bytes,
            "replacement_size_bytes": input.replacement_size_bytes,
            "old_artifact_key": input.expected_old_artifact_key,
            "replacement_artifact_key": input.replacement_artifact_key,
            "format": input.format,
            "vcs_tag": input.vcs_tag,
            "vcs_commit": input.vcs_commit,
            "actor_subject": input.actor_subject,
            "reason": input.reason,
        })),
        client_ip_hash: Set(None),
        created_at: Set(now),
    }
    .insert(connection)
    .await
    .map_err(OrmError::from_db_err)?;
    return Ok(());
}

fn repair_old_identity_matches(
    version: &package_version::Model,
    input: &HistoricalDigestRepairInput,
) -> bool {
    return version.sha256 == input.expected_old_sha256
        && version.size_bytes == input.expected_old_size_bytes
        && version.artifact_key == input.expected_old_artifact_key
        && version.format == input.format
        && version.vcs_tag == input.vcs_tag
        && version.vcs_commit == input.vcs_commit;
}

fn validate_historical_repair(input: &HistoricalDigestRepairInput) -> Result<(), OrmError> {
    match input.repair_class {
        HistoricalDigestRepairClass::LegacyGithubArchiveV030 => {}
    }
    required_text("organization slug", &input.org_slug, 64)?;
    required_text("package name", &input.package_name, 128)?;
    required_text("package version", &input.version, 128)?;
    sha256("expected old artifact SHA-256", &input.expected_old_sha256)?;
    sha256("replacement artifact SHA-256", &input.replacement_sha256)?;
    if input.expected_old_sha256 == input.replacement_sha256 {
        return Err(OrmError::policy(
            "historical digest repair requires different old and replacement digests",
        ));
    }
    if input.expected_old_size_bytes <= 0 || input.replacement_size_bytes <= 0 {
        return Err(OrmError::policy(
            "historical digest repair sizes must be positive",
        ));
    }
    one_of("archive format", &input.format, ARCHIVE_FORMATS)?;
    optional_text("VCS tag", input.vcs_tag.as_deref(), 160)?;
    optional_text("VCS commit", input.vcs_commit.as_deref(), 120)?;
    required_text(
        "expected old artifact key",
        &input.expected_old_artifact_key,
        1_024,
    )?;
    required_text(
        "replacement artifact key",
        &input.replacement_artifact_key,
        1_024,
    )?;
    let expected_old_key = format!("artifacts/{}.{}", input.expected_old_sha256, input.format);
    if input.expected_old_artifact_key != expected_old_key {
        return Err(OrmError::policy(
            "expected old artifact key must be the canonical content-addressed key",
        ));
    }
    let expected_replacement_key =
        format!("artifacts/{}.{}", input.replacement_sha256, input.format);
    if input.replacement_artifact_key != expected_replacement_key {
        return Err(OrmError::policy(
            "replacement artifact key must be the canonical content-addressed key",
        ));
    }
    log_safe_text("admin actor subject", &input.actor_subject, 1, 256)?;
    log_safe_text("historical digest repair reason", &input.reason, 8, 500)?;
    return Ok(());
}

fn log_safe_text(field: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), OrmError> {
    if value.len() < minimum
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(OrmError::policy(format!(
            "{field} must contain {minimum} to {maximum} trimmed, log-safe bytes"
        )));
    }
    return Ok(());
}

fn required_text(field: &str, value: &str, maximum: usize) -> Result<(), OrmError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(OrmError::policy(format!(
            "{field} is required and must be at most {maximum} bytes"
        )));
    }
    return Ok(());
}

fn optional_text(field: &str, value: Option<&str>, maximum: usize) -> Result<(), OrmError> {
    if value.is_some_and(|candidate| candidate.len() > maximum) {
        return Err(OrmError::policy(format!(
            "{field} must be at most {maximum} bytes"
        )));
    }
    return Ok(());
}

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), OrmError> {
    if !allowed.contains(&value) {
        return Err(OrmError::policy(format!(
            "{field} must be one of {}",
            allowed.join(", ")
        )));
    }
    return Ok(());
}

fn sha256(field: &str, value: &str) -> Result<(), OrmError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OrmError::policy(format!(
            "{field} must be 64 lowercase hexadecimal characters"
        )));
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repair_input() -> HistoricalDigestRepairInput {
        return HistoricalDigestRepairInput {
            repair_class: HistoricalDigestRepairClass::LegacyGithubArchiveV030,
            org_slug: "zed-pkg".to_owned(),
            package_name: "zed-orm-core".to_owned(),
            version: "0.1.0".to_owned(),
            expected_old_sha256: "a".repeat(64),
            expected_old_size_bytes: 100,
            expected_old_artifact_key: format!("artifacts/{}.tar.gz", "a".repeat(64)),
            replacement_sha256: "b".repeat(64),
            replacement_size_bytes: 120,
            replacement_artifact_key: format!("artifacts/{}.tar.gz", "b".repeat(64)),
            format: "tar.gz".to_owned(),
            vcs_tag: Some("v0.1.0".to_owned()),
            vcs_commit: Some("c".repeat(40)),
            actor_subject: "admin:shared-auth-subject".to_owned(),
            reason: "repair deterministic packer split-brain".to_owned(),
        };
    }

    fn version_for(input: &HistoricalDigestRepairInput) -> package_version::Model {
        return package_version::Model {
            id: Uuid::nil(),
            package_id: Uuid::nil(),
            version: input.version.clone(),
            version_scheme: "semver".to_owned(),
            sha256: input.expected_old_sha256.clone(),
            size_bytes: input.expected_old_size_bytes,
            format: input.format.clone(),
            vcs_tag: input.vcs_tag.clone(),
            vcs_commit: input.vcs_commit.clone(),
            artifact_key: input.expected_old_artifact_key.clone(),
            manifest: serde_json::json!({"package": {"name": "zed-orm-core"}}),
            download_count: 0,
            yanked: false,
            yanked_at: None,
            yanked_reason: None,
            published_by_user_id: None,
            published_at: chrono::Utc::now().fixed_offset(),
        };
    }

    #[test]
    fn historical_repair_is_a_narrow_compare_and_swap_contract() {
        let input = repair_input();
        assert!(validate_historical_repair(&input).is_ok());
        let version = version_for(&input);
        assert!(repair_old_identity_matches(&version, &input));

        let wrong_old = HistoricalDigestRepairInput {
            expected_old_sha256: "d".repeat(64),
            ..input.clone()
        };
        assert!(!repair_old_identity_matches(&version, &wrong_old));
    }

    #[test]
    fn historical_repair_rejects_generic_overwrite_shapes() {
        let input = repair_input();
        let same_digest = HistoricalDigestRepairInput {
            replacement_sha256: input.expected_old_sha256.clone(),
            ..input.clone()
        };
        assert!(validate_historical_repair(&same_digest).is_err());

        let wrong_old_key = HistoricalDigestRepairInput {
            expected_old_artifact_key: "github/guessable/old.tar.gz".to_owned(),
            ..input.clone()
        };
        assert!(validate_historical_repair(&wrong_old_key).is_err());

        let wrong_new_key = HistoricalDigestRepairInput {
            replacement_artifact_key: "github/guessable/path.tar.gz".to_owned(),
            ..input.clone()
        };
        assert!(validate_historical_repair(&wrong_new_key).is_err());

        let log_unsafe_actor = HistoricalDigestRepairInput {
            actor_subject: "admin\nsubject".to_owned(),
            ..input.clone()
        };
        assert!(validate_historical_repair(&log_unsafe_actor).is_err());

        let short_reason = HistoricalDigestRepairInput {
            reason: "short".to_owned(),
            ..input
        };
        assert!(validate_historical_repair(&short_reason).is_err());
    }
}
